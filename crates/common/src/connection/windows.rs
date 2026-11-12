use super::Accepted;
use anyhow::{Context, Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr::null_mut,
    sync::OnceLock,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::*,
        Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
        *,
    },
    Storage::FileSystem::{CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct LocalMemory(*mut std::ffi::c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

// The endpoint contains an authentication secret, so never rely on inherited ACLs.
fn private_path(path: &Path, directory: bool) -> Result<()> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length);
        let mut user = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            user.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut sid_text = null_mut();
        if ConvertSidToStringSidW(sid, &mut sid_text) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let _sid_text = LocalMemory(sid_text.cast());
        let mut count = 0;
        while *sid_text.add(count) != 0 {
            count += 1;
        }
        let sid_text = String::from_utf16(std::slice::from_raw_parts(sid_text, count))?;
        let sddl = wide(std::ffi::OsStr::new(&format!(
            "O:{sid_text}D:P(A;OICI;FA;;;{sid_text})"
        )));
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let _descriptor = LocalMemory(descriptor);
        let name = wide(path.as_os_str());
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        if directory
            && CreateDirectoryW(name.as_ptr(), &attributes) == 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if (directory && !metadata.is_dir())
            || (!directory && !metadata.is_file())
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            bail!(
                "{} must not be a reparse point or unexpected file type",
                path.display()
            );
        }
        let mut acl = null_mut();
        let mut owner = null_mut();
        let mut existing = null_mut();
        let error = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut existing,
        );
        let _existing = LocalMemory(existing);
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32).into());
        }
        if EqualSid(owner, sid) == 0
            && IsWellKnownSid(owner, WinBuiltinAdministratorsSid) == 0
            && IsWellKnownSid(owner, WinLocalSystemSid) == 0
        {
            bail!("{} is not owned by this user", path.display());
        }
        if !directory {
            if acl.is_null() || (*acl).AceCount == 0 {
                bail!("endpoint has no private ACL");
            }
            for index in 0..(*acl).AceCount {
                let mut ace = null_mut();
                if GetAce(acl, u32::from(index), &mut ace) == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let header = &*ace.cast::<ACE_HEADER>();
                if header.AceType != 0
                    || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
                {
                    bail!("endpoint ACL must contain only user access grants");
                }
                let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                if EqualSid(std::ptr::addr_of!(ace.SidStart).cast_mut().cast(), sid) == 0 {
                    bail!("endpoint ACL must grant access only to this user");
                }
            }
            return Ok(());
        }
        if SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub fn socket_path() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("LOCALAPPDATA or USERPROFILE is required")?;
    let directory = PathBuf::from(base).join("scytty");
    private_path(&directory, true)?;
    Ok(directory.join("server.endpoint"))
}

pub fn server_executable() -> Result<PathBuf> {
    let sibling = std::env::current_exe()?.with_file_name("scytty-server.exe");
    Ok(if sibling.is_file() {
        sibling
    } else {
        PathBuf::from("scytty-server.exe")
    })
}

fn random() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    if unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        bail!("Windows random number generator failed");
    }
    Ok(bytes)
}

fn authenticator(secret: &[u8; 32], nonce: &[u8], role: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts a 32-byte key");
    mac.update(nonce);
    mac.update(role);
    mac
}

fn proof(secret: &[u8; 32], nonce: &[u8], role: &[u8]) -> [u8; 32] {
    authenticator(secret, nonce, role)
        .finalize()
        .into_bytes()
        .into()
}

struct Server {
    secret: [u8; 32],
    digest: [u8; 32],
}
static SERVER: OnceLock<Server> = OnceLock::new();

pub fn executable_digest() -> Result<[u8; 32]> {
    Ok(Sha256::digest(std::fs::read(std::env::current_exe()?)?).into())
}

pub fn listen() -> Result<(File, TcpListener)> {
    let path = socket_path()?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.try_lock().context("server already running")?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let secret = random()?;
    SERVER
        .set(Server {
            secret,
            digest: executable_digest()?,
        })
        .map_err(|_| anyhow::anyhow!("server already initialized"))?;
    let temporary = path.with_extension("new");
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .or_else(|error| {
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error);
            }
            std::fs::remove_file(&temporary)?;
            File::options()
                .write(true)
                .create_new(true)
                .open(&temporary)
        })?;
    file.write_all(&listener.local_addr()?.port().to_le_bytes())?;
    file.write_all(&secret)?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    std::fs::rename(temporary, path)?;
    Ok((lock, listener))
}

fn authenticated() -> Result<Option<TcpStream>> {
    let path = socket_path()?;
    match private_path(&path, false) {
        Ok(()) => {}
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    let endpoint = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if endpoint.len() != 34 {
        bail!("invalid server endpoint {}", path.display());
    }
    let port = u16::from_le_bytes(endpoint[..2].try_into()?);
    let secret: [u8; 32] = endpoint[2..].try_into()?;
    let mut stream = match TcpStream::connect_timeout(
        &(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_secs(1),
    ) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut nonce = [0; 64];
    nonce[..32].copy_from_slice(&random()?);
    stream.write_all(&nonce[..32])?;
    stream.read_exact(&mut nonce[32..])?;
    let mut response = [0; 32];
    stream.read_exact(&mut response)?;
    if authenticator(&secret, &nonce, b"server")
        .verify_slice(&response)
        .is_err()
    {
        bail!("server authentication failed");
    }
    stream.write_all(&proof(&secret, &nonce, b"client"))?;
    Ok(Some(stream))
}

pub fn connect() -> Result<TcpStream> {
    if let Some(mut stream) = authenticated()? {
        stream.write_all(b"B")?;
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        return Ok(stream);
    }
    use std::os::windows::process::CommandExt;
    let path = socket_path()?;
    let log = File::options()
        .create(true)
        .append(true)
        .open(path.with_extension("log"))?;
    let mut child = Command::new(server_executable()?)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("cannot start scytty-server.exe")?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    for _ in 0..100 {
        if let Some(mut stream) = authenticated()? {
            stream.write_all(b"B")?;
            stream.set_read_timeout(None)?;
            stream.set_write_timeout(None)?;
            return Ok(stream);
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!(
        "server did not start; see {}",
        path.with_extension("log").display()
    )
}

/// Authenticate before the shared protocol; control requests never become session clients.
pub fn accept(stream: &mut TcpStream) -> Result<Accepted> {
    let server = SERVER.get().context("server is not initialized")?;
    let mut nonce = [0; 64];
    stream.read_exact(&mut nonce[..32])?;
    nonce[32..].copy_from_slice(&random()?);
    stream.write_all(&nonce[32..])?;
    stream.write_all(&proof(&server.secret, &nonce, b"server"))?;
    let mut response = [0; 32];
    stream.read_exact(&mut response)?;
    if authenticator(&server.secret, &nonce, b"client")
        .verify_slice(&response)
        .is_err()
    {
        bail!("client authentication failed");
    }
    let mut operation = [0];
    stream.read_exact(&mut operation)?;
    match operation[0] {
        b'B' => Ok(Accepted::Session),
        b'I' => {
            stream.write_all(&std::process::id().to_le_bytes())?;
            stream.write_all(&server.digest)?;
            Ok(Accepted::Control)
        }
        b'R' => {
            let mut expected = [0; 36];
            stream.read_exact(&mut expected)?;
            if expected[..4] != std::process::id().to_le_bytes() || expected[4..] != server.digest {
                stream.write_all(b"N")?;
                return Ok(Accepted::Control);
            }
            stream.write_all(b"Y")?;
            // Let the session loop drop PTYs and kill shells before this daemon exits.
            Ok(Accepted::Restart)
        }
        _ => bail!("invalid daemon operation"),
    }
}

pub fn daemon_status() -> Result<Option<(u32, [u8; 32])>> {
    let Some(mut stream) = authenticated()? else {
        return Ok(None);
    };
    stream.write_all(b"I")?;
    let mut bytes = [0; 36];
    stream.read_exact(&mut bytes)?;
    Ok(Some((
        u32::from_le_bytes(bytes[..4].try_into()?),
        bytes[4..].try_into()?,
    )))
}

pub fn restart_daemon(pid: u32, digest: [u8; 32]) -> Result<()> {
    let Some(mut stream) = authenticated()? else {
        return Ok(());
    };
    stream.write_all(b"R")?;
    stream.write_all(&pid.to_le_bytes())?;
    stream.write_all(&digest)?;
    let mut response = [0];
    stream.read_exact(&mut response)?;
    if response != *b"Y" {
        bail!("daemon changed since confirmation; reconnect and confirm again");
    }
    for _ in 0..100 {
        match daemon_status() {
            Ok(Some((running, _))) if running == pid => {}
            Ok(_) => return Ok(()),
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::BrokenPipe
                    )
                }) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!("daemon did not stop")
}

#[cfg(test)]
#[path = "../../tests/unit/connection/windows.rs"]
mod tests;
