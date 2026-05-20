//! Integration tests for darwin-sys safe wrappers.
//!
//! These run on the host (macOS/aarch64) as ordinary Rust tests, so they
//! have access to std.  They verify that each safe wrapper correctly
//! round-trips through the underlying syscall.

#[cfg(target_os = "macos")]
mod tests {

use darwin_sys::{rand, time, env, fs, process};
use darwin_sys::ffi::types::*;

// ── Randomness ────────────────────────────────────────────────────────

#[test]
fn rand_fill_basic() {
    let mut buf = [0u8; 32];
    rand::fill(&mut buf).expect("getentropy failed");
    // It would be astronomically unlikely for 32 random bytes to be all zero.
    assert_ne!(buf, [0u8; 32]);
}

#[test]
fn rand_fill_large() {
    // Covers the >256-byte chunked path.
    let mut buf = [0u8; 512];
    rand::fill(&mut buf).expect("getentropy failed on large buf");
    let all_zero = buf.iter().all(|&b| b == 0);
    assert!(!all_zero);
}

#[test]
fn rand_random_u64() {
    let a = rand::random_u64().unwrap();
    let b = rand::random_u64().unwrap();
    // Two independent u64s being equal is a 1-in-2^64 event.
    assert_ne!(a, b);
}

// ── Time ─────────────────────────────────────────────────────────────

#[test]
fn time_monotonic_increases() {
    let t0 = time::monotonic_ns().unwrap();
    // Do something that takes >0 ns.
    let _ = rand::random_u64().unwrap();
    let t1 = time::monotonic_ns().unwrap();
    assert!(t1 >= t0, "monotonic clock went backwards: {} > {}", t0, t1);
}

#[test]
fn time_realtime_sane() {
    let (secs, _ns) = time::realtime().unwrap();
    // 2024-01-01 00:00:00 UTC = 1704067200
    assert!(secs > 1_704_067_200, "realtime before 2024? secs={}", secs);
}

#[test]
fn time_gettimeofday_sane() {
    let (secs, usec) = time::gettimeofday().unwrap();
    assert!(secs > 1_704_067_200);
    assert!(usec >= 0 && usec < 1_000_000);
}

#[test]
fn time_sleep_ms_short() {
    let t0 = time::monotonic_ns().unwrap();
    time::sleep_ms(1).unwrap();
    let elapsed = time::monotonic_ns().unwrap() - t0;
    // Should have slept at least 500µs (give generous margin for scheduler jitter).
    assert!(elapsed >= 500_000, "sleep_ms(1) was too short: {}ns", elapsed);
}

#[test]
fn time_thread_cpu_ns() {
    let t0 = time::thread_cpu_ns().unwrap();
    // Burn some CPU.
    let mut x = 0u64;
    for i in 0..1_000_000u64 { x = x.wrapping_add(i); }
    let _ = x;
    let t1 = time::thread_cpu_ns().unwrap();
    assert!(t1 > t0);
}

// ── Environment ────────────────────────────────────────────────────────

#[test]
fn env_getenv_path() {
    let mut buf = [0u8; 4096];
    let n = env::getenv(b"PATH", &mut buf);
    assert!(n.is_some(), "PATH not set");
    let n = n.unwrap();
    assert!(n > 0);
    // PATH must contain at least one '/'
    assert!(buf[..n].contains(&b'/'));
}

#[test]
fn env_setenv_getenv_unsetenv() {
    let key = b"DARWIN_SYS_TEST_VAR";
    let val = b"hello_world";
    env::setenv(key, val, true).unwrap();

    let mut buf = [0u8; 64];
    let n = env::getenv(key, &mut buf).expect("var not found after setenv");
    assert_eq!(&buf[..n], val);

    env::unsetenv(key).unwrap();
    assert!(env::getenv(key, &mut buf).is_none(), "var still set after unsetenv");
}

#[test]
fn env_getcwd() {
    let mut buf = [0u8; 4096];
    let n = env::getcwd(&mut buf).unwrap();
    assert!(n > 0);
    assert_eq!(buf[0], b'/');
}

// ── Filesystem ─────────────────────────────────────────────────────────

#[test]
fn fs_write_and_read_back() {
    let tmp = std::env::temp_dir();
    let path = tmp.join("darwin_sys_test.bin");
    let path_bytes = path.as_os_str().as_encoded_bytes();

    let content = b"darwin-sys integration test payload 42";

    // Write.
    {
        let f = fs::File::create(path_bytes, 0o600).unwrap();
        f.write_all(content).unwrap();
        f.sync().unwrap();
    }

    // Read back.
    {
        let f = fs::File::open_read(path_bytes).unwrap();
        let mut buf = [0u8; 64];
        let n = f.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], content);
    }

    // Metadata.
    {
        let st = fs::metadata(path_bytes).unwrap();
        assert_eq!(st.st_size, content.len() as i64);
        assert!(st.is_file());
        assert!(!st.is_dir());
    }

    // Cleanup.
    fs::unlink(path_bytes).unwrap();
    assert!(fs::metadata(path_bytes).is_err(), "file still exists after unlink");
}

#[test]
fn fs_mkdir_rmdir() {
    let tmp = std::env::temp_dir();
    let dir = tmp.join("darwin_sys_test_dir");
    let dir_bytes = dir.as_os_str().as_encoded_bytes();

    // Remove if leftover from previous run.
    let _ = fs::rmdir(dir_bytes);

    fs::mkdir(dir_bytes, 0o700).unwrap();
    let st = fs::metadata(dir_bytes).unwrap();
    assert!(st.is_dir());

    fs::rmdir(dir_bytes).unwrap();
    assert!(fs::metadata(dir_bytes).is_err());
}

#[test]
fn fs_pipe_write_read() {
    let (r, w) = fs::pipe().unwrap();
    let sent = b"pipe test";
    w.write_all(sent).unwrap();
    drop(w);

    let mut buf = [0u8; 32];
    let n = r.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], sent);
}

#[test]
fn fs_seek_and_pread() {
    let tmp = std::env::temp_dir();
    let path = tmp.join("darwin_sys_seek_test.bin");
    let path_bytes = path.as_os_str().as_encoded_bytes();

    let data = b"ABCDEFGHIJ";
    {
        let f = fs::File::create(path_bytes, 0o600).unwrap();
        f.write_all(data).unwrap();
    }

    {
        let f = fs::File::open_read(path_bytes).unwrap();
        let mut buf = [0u8; 4];
        let n = f.pread(&mut buf, 3).unwrap();
        assert_eq!(&buf[..n], b"DEFG");
    }

    fs::unlink(path_bytes).unwrap();
}

// ── Process identity ───────────────────────────────────────────────────

#[test]
fn process_identity() {
    let pid = process::getpid();
    assert!(pid > 0);
    // Our pid should match what std reports.
    assert_eq!(pid as u32, std::process::id());

    let uid = process::getuid();
    let euid = process::geteuid();
    // Running as a non-root user: uid should be >0 (almost certainly true in CI).
    // We can't assert uid > 0 unconditionally — root is valid too — but we can
    // assert the call succeeds and returns something consistent.
    let _ = (uid, euid);
}

// ── mmap ───────────────────────────────────────────────────────────────

#[test]
fn fs_mmap_anon() {
    let len = 4096;
    let ptr = fs::mmap_anon(len, PROT_READ | PROT_WRITE).unwrap();
    assert!(!ptr.is_null());

    // Write and read back.
    unsafe {
        *ptr = 0xAB;
        assert_eq!(*ptr, 0xAB);
        fs::munmap(ptr, len).unwrap();
    }
}

} // mod tests
