//! Bounded local socket I/O shared by native validation and optional presentation.
use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::Path,
    time::Instant,
};

pub(super) fn connect_peer(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    if Instant::now() >= deadline {
        return Err(io::Error::other("local socket connect deadline reached"));
    }
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io::Error::other("invalid local socket path"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, value) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *value as libc::c_char;
    }
    let size =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    address.sun_len = size as u8;
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    let enabled: libc::c_int = 1;
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&enabled as *const libc::c_int).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t,
            )
        } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), size) } != 0 {
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS) | Some(libc::EAGAIN)
        ) {
            return Err(error);
        }
        wait_socket(&stream, libc::POLLOUT, deadline)?;
        let mut error: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&error) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut error as *mut libc::c_int).cast(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
    }
    Ok(stream)
}
pub(super) fn wait_socket(
    stream: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> io::Result<()> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "local socket deadline",
            ));
        }
        let mut fd = libc::pollfd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        };
        let milliseconds = left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(&mut fd, 1, milliseconds) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::last_os_error());
        }
    }
}
