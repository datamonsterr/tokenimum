#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use tkm_proto::encode_frame;

    use super::*;

    fn paths(home: &tempfile::TempDir) -> Paths {
        // A short path also stays within macOS's Unix socket path limit.
        let mut paths = Paths::from_dirs(home.path(), None, None, None);
        paths.socket = home.path().join("tkmd.sock");
        paths
    }

    #[test]
    fn probe_missing_socket_is_down() {
        let home = tempfile::tempdir().unwrap();
        assert!(matches!(probe(&paths(&home)), DaemonState::Down { reason } if !reason.is_empty()));
    }

    #[test]
    fn probe_reads_monitor_hello() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener = UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let frame = encode_frame(
                &MonitorMessage::Hello { version: PROTOCOL_VERSION },
                MONITOR_MAX_FRAME,
            )
            .unwrap();
            stream.write_all(&frame).unwrap();
        });
        let state = probe(&paths);
        peer.join().unwrap();
        assert_eq!(state, DaemonState::Running { version: PROTOCOL_VERSION });
    }

    #[test]
    fn probe_rejects_incompatible_protocol() {
        assert_rejected_frame(
            &encode_frame(
                &MonitorMessage::Hello { version: PROTOCOL_VERSION + 1 },
                MONITOR_MAX_FRAME,
            )
            .unwrap(),
        );
    }

    #[test]
    fn probe_rejects_oversize_header() {
        assert_rejected_frame(&u32::try_from(MONITOR_MAX_FRAME + 1).unwrap().to_be_bytes());
    }

    #[test]
    fn probe_rejects_malformed_body() {
        assert_rejected_frame(b"\0\0\0\x01{");
    }

    #[test]
    fn probe_rejects_truncated_frame() {
        assert_rejected_frame(b"\0\0");
    }

    #[test]
    fn probe_rejects_non_hello_frame() {
        let message = MonitorMessage::Spend(tkm_proto::monitor::SpendUpdate {
            today_usd: 0.0,
            daily_cap_usd: 1.0,
            session_cap_usd: 0.1,
        });
        assert_rejected_frame(&encode_frame(&message, MONITOR_MAX_FRAME).unwrap());
    }

    fn assert_rejected_frame(frame: &[u8]) {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener = UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let frame = frame.to_vec();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&frame).unwrap();
        });
        let state = probe(&paths);
        peer.join().unwrap();
        assert!(matches!(state, DaemonState::Down { reason } if !reason.is_empty()));
    }

    #[test]
    fn probe_times_out_waiting_for_hello() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener = UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let (release, wait) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            // The probe must have closed its side when its deadline elapsed.
            let mut byte = [0];
            stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        });
        let state = probe(&paths);
        release.send(()).unwrap();
        peer.join().unwrap();
        assert!(matches!(state, DaemonState::Down { reason } if reason.contains("timed out")));
    }

    #[test]
    fn open_db_missing_file_is_an_error_without_creation() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let error = match open_db(&paths) {
            Ok(_) => panic!("missing database must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains(&paths.db.display().to_string()));
        assert_eq!(error.exit_code(), std::process::ExitCode::FAILURE);
        assert!(!paths.data_dir.exists());
    }

    #[test]
    fn open_db_is_read_only_beside_writer() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let _writer = open_db_for_write(&paths).unwrap();
        let reader = open_db(&paths).unwrap();
        assert!(reader.execute_batch("CREATE TABLE forbidden (id INTEGER)").is_err());
        let version: i64 = reader.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert!(version > 0);
    }
}
