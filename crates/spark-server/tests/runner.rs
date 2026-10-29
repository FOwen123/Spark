//! Its own test binary: it points PATH at a fake agent CLI.

#[cfg(unix)]
#[tokio::test]
async fn stop_ends_everything_the_agent_started() {
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("worker.pid");
    let exe = dir.path().join("codex");
    let script = format!("#!/bin/sh\n/bin/sleep 120 &\necho $! > {}\nwait\n", pid_file.display());
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    // SAFETY: this binary has one test, so nothing else reads the environment meanwhile.
    unsafe { std::env::set_var("PATH", dir.path()) };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let id = spark_server::runner::start("codex", "hi", "File", "id", dir.path(), tx).unwrap();
    let pid = loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(p) = std::fs::read_to_string(&pid_file).map(|p| p.trim().to_string())
            && !p.is_empty()
        {
            break p;
        }
    };
    assert!(spark_server::runner::stop(id));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let alive = std::process::Command::new("/bin/kill").args(["-0", &pid]).status().unwrap().success();
    if alive {
        std::process::Command::new("/bin/kill").args(["-KILL", &pid]).status().unwrap();
    }
    assert!(!alive, "the agent's own subprocess stops too");
}
