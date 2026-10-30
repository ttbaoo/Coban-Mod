use super::{Reply, Request};
use crate::storage;
use anyhow::{Context, Result, ensure};
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{
    ConnectOptions, Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*,
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
// A bounded layout (1.5 MB) may accompany both 160×80 and 640×320 PNG previews.
// JSON encodes PNG bytes as numbers; allow room for all three together.
const MAX_FRAME: usize = 8_000_000;

fn runtime_dir(data: &Path) -> Result<PathBuf> {
    fs::create_dir_all(data)?;
    let canonical = fs::canonicalize(data)?;
    #[cfg(unix)]
    let key = hex::encode(Sha256::digest(canonical.to_string_lossy().as_bytes()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let path = PathBuf::from(format!(
            "/tmp/mk-{}-{}",
            unsafe { libc::geteuid() },
            &key[..16]
        ));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.uid() == unsafe { libc::geteuid() }
                && meta.mode() & 0o077 == 0,
            "Thư mục IPC không thuộc user hoặc không riêng tư"
        );
        Ok(path)
    }
    #[cfg(windows)]
    {
        let path = canonical.join("runtime");
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}
fn endpoint(data: &Path) -> Result<Name<'static>> {
    #[cfg(unix)]
    {
        Ok(runtime_dir(data)?
            .join("agent.sock")
            .to_fs_name::<GenericFilePath>()?
            .into_owned())
    }
    #[cfg(windows)]
    {
        let mut session = 0;
        unsafe {
            windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(
                std::process::id(),
                &mut session,
            );
        }
        let key = hex::encode(Sha256::digest(
            format!("{}:{}:{session}", runtime_dir(data)?.display(), user_sid()?).as_bytes(),
        ));
        Ok(format!("vn.baotruong.mk.{}", &key[..24])
            .to_ns_name::<GenericNamespaced>()?
            .into_owned())
    }
}
pub fn listener() -> Result<Option<(Listener, File)>> {
    let data = storage::data_dir();
    let data = data.as_path();
    let dir = runtime_dir(data)?;
    let mut opts = OpenOptions::new();
    opts.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let lock = opts.open(dir.join("agent.lock"))?;
    if let Err(error) = lock.try_lock() {
        if matches!(error, std::fs::TryLockError::WouldBlock) {
            return Ok(None);
        }
        return Err(error.into());
    }
    #[cfg(unix)]
    {
        // Holding the OS lock proves the previous owner exited. Never unlink a live listener.
        match fs::remove_file(dir.join("agent.sock")) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    let options = ListenerOptions::new()
        .name(endpoint(data)?)
        .nonblocking(ListenerNonblockingMode::Both);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;{})", user_sid()?))?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    Ok(Some((options.create_sync()?, lock)))
}
fn authenticate(stream: &Stream) -> Result<()> {
    #[cfg(unix)]
    ensure!(
        stream.peer_creds()?.euid() == Some(unsafe { libc::geteuid() }),
        "IPC từ user khác bị từ chối"
    );
    #[cfg(windows)]
    {
        let pid = stream.peer_creds()?.pid().context("IPC không có PID")?;
        ensure!(
            process_sid(pid)? == user_sid()?,
            "IPC từ user khác bị từ chối"
        );
    }
    Ok(())
}
pub fn connect_at(data: &Path) -> Result<Stream> {
    let stream = ConnectOptions::new()
        .name(endpoint(data)?)
        .wait_mode(interprocess::ConnectWaitMode::Timeout(
            Duration::from_millis(300),
        ))
        .nonblocking_stream(true)
        .connect_sync()?;
    authenticate(&stream)?;
    Ok(stream)
}
pub fn receive_request(stream: &mut Stream) -> Result<Request> {
    authenticate(stream)?;
    read_frame(stream, Duration::from_secs(3))
}
pub fn send_reply(stream: &mut Stream, reply: &Reply) -> Result<()> {
    write_frame(stream, reply, Duration::from_secs(3))
}
pub fn exchange_at(request: &Request, data: &Path) -> Result<Reply> {
    let mut stream = connect_at(data)?;
    write_frame(&mut stream, request, Duration::from_secs(3))?;
    read_frame(
        &mut stream,
        if matches!(
            request.job,
            super::Job::Hello | super::Job::Status | super::Job::Profiles
        ) {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(70)
        },
    )
    .context(
        "Agent không trả lời; không tự gửi lại thao tác. Đọc lại trạng thái trước khi chỉnh tiếp",
    )
}
fn transfer(
    mut action: impl FnMut() -> std::io::Result<usize>,
    deadline: Instant,
) -> Result<usize> {
    loop {
        ensure!(Instant::now() < deadline, "IPC hết thời gian");
        match action() {
            Ok(0) => anyhow::bail!("IPC đã đóng"),
            Ok(n) => return Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                ensure!(Instant::now() < deadline, "IPC hết thời gian");
                std::thread::sleep(Duration::from_millis(3));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
}
fn read_exact(stream: &mut impl Read, mut buf: &mut [u8], deadline: Instant) -> Result<()> {
    while !buf.is_empty() {
        let n = transfer(|| stream.read(buf), deadline)?;
        buf = &mut buf[n..];
    }
    Ok(())
}
fn read_frame<T: DeserializeOwned>(stream: &mut impl Read, timeout: Duration) -> Result<T> {
    let deadline = Instant::now() + timeout;
    let mut header = [0u8; 4];
    read_exact(stream, &mut header, deadline)?;
    let len = u32::from_be_bytes(header) as usize;
    ensure!(len > 0 && len <= MAX_FRAME, "Kích thước IPC không hợp lệ");
    let mut bytes = vec![0; len];
    read_exact(stream, &mut bytes, deadline)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn write_frame(stream: &mut impl Write, value: &impl Serialize, timeout: Duration) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_FRAME, "Nội dung IPC quá lớn");
    let mut frame = Vec::with_capacity(bytes.len() + 4);
    frame.extend((bytes.len() as u32).to_be_bytes());
    frame.extend(bytes);
    let deadline = Instant::now() + timeout;
    let mut remaining = frame.as_slice();
    while !remaining.is_empty() {
        let n = transfer(|| stream.write(remaining), deadline)?;
        remaining = &remaining[n..];
    }
    Ok(())
}
#[cfg(windows)]
fn user_sid() -> Result<String> {
    process_sid(std::process::id())
}
#[cfg(windows)]
fn process_sid(pid: u32) -> Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        ensure!(!process.is_null(), "Không đọc được IPC peer");
        let mut token = std::ptr::null_mut();
        let ok = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        CloseHandle(process);
        ensure!(ok != 0, "Không đọc được user IPC peer");
        let mut len = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        ensure!(
            len as usize >= std::mem::size_of::<TOKEN_USER>(),
            "SID trống"
        );
        let mut storage = vec![0u64; (len as usize).div_ceil(8)];
        let ok = GetTokenInformation(token, TokenUser, storage.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        ensure!(ok != 0, "Không đọc được SID");
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        ensure!(
            ConvertSidToStringSidW(user.User.Sid, &mut text) != 0,
            "SID không hợp lệ"
        );
        let mut size = 0;
        while *text.add(size) != 0 {
            size += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, size));
        LocalFree(text.cast());
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_frames_are_rejected_before_allocation() {
        let mut bytes = std::io::Cursor::new((MAX_FRAME as u32 + 1).to_be_bytes());
        assert!(read_frame::<serde_json::Value>(&mut bytes, Duration::from_secs(1)).is_err());
    }
    #[test]
    fn framing_handles_fragmented_reads() {
        struct OneByte(std::io::Cursor<Vec<u8>>);
        impl Read for OneByte {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(&mut buf[..1])
            }
        }
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &vec![1, 2, 3], Duration::from_secs(1)).unwrap();
        let value: Vec<u32> = read_frame(
            &mut OneByte(std::io::Cursor::new(bytes)),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(value, vec![1, 2, 3]);
    }
}
