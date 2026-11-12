use super::*;

#[test]
fn executable_validation() {
    for bin in ["/usr/bin/fish", "htop", "C:\\Program Files\\app.exe"] {
        assert!(validate_bin(bin).is_ok());
    }
    for bin in ["", "  ", "bad\nprogram", "bad\0program"] {
        assert!(validate_bin(bin).is_err());
    }
    assert!(validate_bin(&"x".repeat(4097)).is_err());
}

#[test]
fn handshake_then_frames() -> Result<()> {
    let (mut client, mut server) = crate::stream::pair()?;
    let peer = std::thread::spawn(move || -> Result<()> {
        handshake(&mut server)?;
        assert!(matches!(receive::<Request>(&mut server)?, Request::NewTab));
        Ok(())
    });
    handshake(&mut client)?;
    send(&mut client, &Request::NewTab)?;
    peer.join().unwrap()?;
    Ok(())
}

#[test]
fn handshake_rejects_mismatch_and_legacy() -> Result<()> {
    for kind in ["version", "magic", "legacy"] {
        let (mut client, mut server) = crate::stream::pair()?;
        let peer = std::thread::spawn(move || {
            let mut hello = [0; 12];
            server.read_exact(&mut hello).unwrap();
            match kind {
                "version" => hello[8..].copy_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes()),
                "magic" => hello[0] = b'X',
                _ => return,
            }
            server.write_all(&hello).unwrap();
        });
        let error = format!("{:#}", handshake(&mut client).unwrap_err());
        assert!(
            error.contains(match kind {
                "version" => "protocol mismatch",
                "magic" => "missing protocol handshake",
                _ => "older server",
            }),
            "{error}"
        );
        peer.join().unwrap();
    }
    Ok(())
}

#[test]
fn framing() -> Result<()> {
    let mut bytes = Vec::new();
    send(
        &mut bytes,
        &Request::Input {
            id: 3,
            bytes: vec![0, 255],
        },
    )?;
    assert!(
        matches!(receive(&mut &bytes[..])?, Request::Input { id: 3, bytes } if bytes == [0, 255])
    );
    assert!(receive::<Request>(&mut &u32::MAX.to_be_bytes()[..]).is_err());
    assert_eq!(&bytes[4..6], &[0x1f, 0x8b]);
    let checksum = bytes.len() - 8;
    bytes[checksum] ^= 0xff;
    assert!(receive::<Request>(&mut &bytes[..]).is_err());
    Ok(())
}

#[test]
fn consecutive_frames() -> Result<()> {
    let mut bytes = Vec::new();
    send(&mut bytes, &Request::NewTab)?;
    send(&mut bytes, &Request::Activate { id: 42 })?;
    let mut reader = &bytes[..];
    assert!(matches!(receive::<Request>(&mut reader)?, Request::NewTab));
    assert!(matches!(
        receive::<Request>(&mut reader)?,
        Request::Activate { id: 42 }
    ));
    assert!(reader.is_empty());
    Ok(())
}
