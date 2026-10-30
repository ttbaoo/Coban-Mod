use mk::transport::Transport;
use mk::{
    protocol::{MockTransport, Protocol},
    screen_layout::{self, Layout},
};

#[test]
fn old_firmware_never_receives_the_unsafe_set_size_reload() {
    struct Old {
        inner: MockTransport,
        packets: Vec<Vec<u8>>,
        flags: Option<u8>,
    }
    impl Transport for Old {
        fn simulated(&self) -> bool {
            true
        }
        fn exchange(&mut self, q: &[u8], echo: usize) -> anyhow::Result<[u8; 32]> {
            self.packets.push(q.to_vec());
            if q.starts_with(&[8, 0, mk::device_layer::VALUE_ID]) {
                let mut r = [0; 32];
                if let Some(flags) = self.flags {
                    r = self.inner.exchange(q, echo)?;
                    r[8] = flags;
                } else {
                    r[..q.len()].copy_from_slice(q);
                }
                return Ok(r);
            }
            self.inner.exchange(q, echo)
        }
    }
    let gif = screen_layout::render(&Layout::default()).unwrap().gif;
    for flags in [None, Some(1), Some(3)] {
        let mut protocol = Protocol::new(Old {
            inner: MockTransport::new(),
            packets: vec![],
            flags,
        });
        protocol.upload_gif(&gif).unwrap();
        let set = protocol
            .io
            .packets
            .iter()
            .any(|q| q.starts_with(&[7, 0, 87]));
        let legacy = protocol
            .io
            .packets
            .iter()
            .any(|q| q.starts_with(&[9, 0, 87]));
        assert_eq!(
            (set, legacy),
            if flags == Some(3) {
                (true, false)
            } else {
                (false, true)
            }
        );
    }
}

struct DisplayTransport {
    inner: MockTransport,
    packets: Vec<Vec<u8>>,
    displayed: Vec<u8>,
}
impl Transport for DisplayTransport {
    fn simulated(&self) -> bool {
        true
    }
    fn exchange(&mut self, request: &[u8], echo: usize) -> anyhow::Result<[u8; 32]> {
        let reply = self.inner.exchange(request, echo)?;
        if request.starts_with(&[7, 0, 87]) {
            let size = u32::from_be_bytes(request[3..7].try_into().unwrap()) as usize;
            self.inner.gif_bytes.truncate(size);
        }
        // The running display reloads on the screen command, even if the mode is unchanged.
        if request.starts_with(&[7, 0, 81, 2]) {
            self.displayed.clone_from(&self.inner.gif_bytes);
        }
        self.packets.push(request.to_vec());
        Ok(reply)
    }
}
#[test]
fn replacing_an_image_in_gif_mode_always_reactivates_the_display_after_flash_save() {
    let transport = DisplayTransport {
        inner: MockTransport::new(),
        packets: Vec::new(),
        displayed: Vec::new(),
    };
    let mut protocol = Protocol::new(transport);
    let before = protocol
        .read_snapshot(protocol.io.inner.snapshot.device.clone())
        .unwrap();
    let mut layout = Layout::default();
    let first = screen_layout::render(&layout).unwrap();
    let applied = protocol.apply_screen_gif(&before, &first.gif).unwrap();
    layout.tiles[0].text = "NEW".into();
    let second = screen_layout::render(&layout).unwrap();
    protocol.io.packets.clear();
    protocol
        .apply_screen_gif(&applied.snapshot, &second.gif)
        .unwrap();
    assert_ne!(first.gif, second.gif);
    assert_eq!(protocol.io.displayed, second.gif);
    let packets = &protocol.io.packets;
    let save = packets.iter().position(|p| p == &[9, 0, 151]).unwrap();
    let activate = packets
        .iter()
        .position(|p| p.starts_with(&[7, 0, 81, 2, 0]))
        .unwrap();
    assert!(save < activate);
    assert_eq!(packets[activate + 1], [9, 0, 150]);
    assert!(!packets.iter().any(|p| [5, 15, 21].contains(&p[0])));
    // The refresh only changes animation/layer overlay; timeout and idle settings survive.
    let screen = applied
        .snapshot
        .settings
        .iter()
        .find(|s| s.key == "screen")
        .unwrap();
    assert_eq!(&packets[activate][5..], &screen.value[2..]);
}
#[test]
fn screen_apply_keeps_keys_and_macros_and_disables_layer_overlay() {
    let mut protocol = Protocol::new(MockTransport::new());
    let before = protocol
        .read_snapshot(protocol.io.snapshot.device.clone())
        .unwrap();
    let rendered = screen_layout::render(&Layout::default()).unwrap();
    assert_eq!(protocol.io.writes, 0);
    let result = protocol.apply_screen_gif(&before, &rendered.gif).unwrap();
    assert_eq!(protocol.io.gif_bytes, rendered.gif);
    assert_eq!(result.snapshot.keymap, before.keymap);
    assert_eq!(result.snapshot.encoders, before.encoders);
    assert_eq!(result.snapshot.macro_buffer, before.macro_buffer);
    assert_eq!(result.snapshot.macro_names, before.macro_names);
    let screen = result
        .snapshot
        .settings
        .iter()
        .find(|s| s.key == "screen")
        .unwrap();
    assert_eq!(&screen.value[..2], &[2, 0]);
    assert!(result.backup.unwrap().exists());
    let writes = protocol.io.writes;
    protocol
        .apply_screen_gif(&result.snapshot, &rendered.gif)
        .unwrap();
    assert!(protocol.io.writes > writes); // image replacement remains explicit even when screen settings are unchanged
}
#[test]
fn drift_and_invalid_image_fail_before_upload_and_partial_write_is_not_replayed() {
    let mut protocol = Protocol::new(MockTransport::new());
    let before = protocol
        .read_snapshot(protocol.io.snapshot.device.clone())
        .unwrap();
    let rendered = screen_layout::render(&Layout::default()).unwrap();
    assert!(protocol.apply_screen_gif(&before, b"bad").is_err());
    assert_eq!(protocol.io.writes, 0);
    protocol.io.snapshot.keymap[2] = 104;
    assert!(protocol.apply_screen_gif(&before, &rendered.gif).is_err());
    assert_eq!(protocol.io.writes, 0);
    protocol.io.snapshot = before.clone();
    protocol.io.fail_after = Some(2);
    assert!(protocol.apply_screen_gif(&before, &rendered.gif).is_err());
    assert_eq!(protocol.io.writes, 2);
    assert!(protocol.io.snapshot.same_content(&before));
}
#[test]
fn clock_tick_resends_the_gif_and_reactivates_without_touching_settings() {
    let transport = DisplayTransport {
        inner: MockTransport::new(),
        packets: Vec::new(),
        displayed: Vec::new(),
    };
    let mut protocol = Protocol::new(transport);
    let before = protocol
        .read_snapshot(protocol.io.inner.snapshot.device.clone())
        .unwrap();
    let mut layout = Layout::default();
    layout.center.kind = screen_layout::Content::Clock;
    let first = screen_layout::render(&layout).unwrap();
    let applied = protocol.apply_screen_gif(&before, &first.gif).unwrap();
    let screen = applied
        .snapshot
        .settings
        .iter()
        .find(|s| s.key == "screen")
        .unwrap()
        .value
        .clone();
    let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 28)
        .unwrap()
        .and_hms_opt(10, 42, 0)
        .unwrap();
    let tick = screen_layout::render_at(&layout, at).unwrap();
    protocol.io.packets.clear();
    protocol.refresh_screen_gif(&screen, &tick.gif).unwrap();
    assert_eq!(protocol.io.displayed, tick.gif);
    let packets = &protocol.io.packets;
    let activate = packets
        .iter()
        .position(|p| p.starts_with(&[7, 0, 81, 2]))
        .unwrap();
    assert_eq!(packets[activate + 1], [9, 0, 150]);
    // GIF writes plus the read-only capability query; no keymap/macro traffic.
    assert!(
        packets
            .iter()
            .all(|p| p[0] == 7 || p[0] == 9 || p.as_slice() == mk::device_layer::request())
    );
    let after = protocol
        .read_snapshot(protocol.io.inner.snapshot.device.clone())
        .unwrap();
    assert!(after.same_content(&applied.snapshot));
    let mut other = screen.clone();
    other[0] = 0;
    assert!(protocol.refresh_screen_gif(&other, &tick.gif).is_err());
}
