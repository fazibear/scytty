#[test]
fn bidirectional_pair() -> std::io::Result<()> {
    use std::io::{Read, Write};
    let (mut a, mut b) = super::pair()?;
    a.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    b.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    a.write_all(b"a")?;
    let mut byte = [0];
    b.read_exact(&mut byte)?;
    assert_eq!(byte, *b"a");
    b.write_all(b"b")?;
    a.read_exact(&mut byte)?;
    assert_eq!(byte, *b"b");
    Ok(())
}
