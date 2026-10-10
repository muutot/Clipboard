//! Data-control event-driven clipboard monitor (Linux only).
//!
//! Signals `ClipboardChange` instead of polling; `clipboard.rs` still performs
//! the actual reads.

// ---------------------------------------------------------------------------
// Data-control event-driven clipboard monitor (Linux only)
// ---------------------------------------------------------------------------

/// Spawns the event-driven clipboard monitor backed by the Wayland
/// data-control protocols (`ext-data-control-unstable-v1`, with the older
/// `wlr-data-control-unstable-v1` as a second choice).
///
/// The monitor thread binds a data-control device for the first seat and sends
/// a [`ClipboardChange`] whenever the compositor reports a new CLIPBOARD
/// selection — no polling. Contents are still read by the capture loop through
/// the regular `wl-paste` helpers; the monitor only signals changes. It blocks
/// in `poll(2)` on the Wayland socket fd plus the read end of a stop pipe, so
/// `stop()` wakes it immediately.
///
/// Returns `None` when the compositor advertises neither manager global (or
/// the connection/seat setup fails); the caller then falls back to the generic
/// poll loop. The returned [`StopPipeWriter`] must be triggered (or dropped)
/// on stop; the monitor thread owns and closes the pipe's read end.
#[cfg(target_os = "linux")]
pub(crate) fn try_spawn_data_control_monitor(
    sender: std::sync::mpsc::Sender<crate::platform::windows::ClipboardChange>,
) -> Option<(
    std::thread::JoinHandle<()>,
    crate::platform::linux::stop_pipe::StopPipeWriter,
)> {
    use crate::platform::linux::stop_pipe::StopPipePair;
    use crate::platform::linux::wayland::dispatch::dispatch_once;
    use wayland_client::protocol::{wl_callback, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols::ext::data_control::v1::client::{
        ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
        ext_data_control_manager_v1::{self, ExtDataControlManagerV1},
        ext_data_control_offer_v1::ExtDataControlOfferV1,
    };
    use wayland_protocols_wlr::data_control::v1::client::{
        zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
        zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
        zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
    };

    let stop = StopPipePair::new().ok()?;

    enum Manager {
        Ext(ExtDataControlManagerV1),
        Wlr(ZwlrDataControlManagerV1),
    }

    impl Manager {
        fn get_data_device(
            &self,
            seat: &wl_seat::WlSeat,
            qh: &QueueHandle<DataControlState>,
        ) -> DataControlDevice {
            match self {
                Manager::Ext(manager) => {
                    DataControlDevice::Ext(manager.get_data_device(seat, qh, ()))
                }
                Manager::Wlr(manager) => {
                    DataControlDevice::Wlr(manager.get_data_device(seat, qh, ()))
                }
            }
        }
    }

    enum DataControlDevice {
        Ext(ExtDataControlDeviceV1),
        Wlr(ZwlrDataControlDeviceV1),
    }

    impl DataControlDevice {
        fn destroy(&self) {
            match self {
                DataControlDevice::Ext(device) => device.destroy(),
                DataControlDevice::Wlr(device) => device.destroy(),
            }
        }
    }

    /// Offers handed out by the compositor. Each new data offer supersedes
    /// the previous ones, so they are destroyed eagerly to keep client and
    /// compositor state bounded.
    enum Offer {
        Ext(ExtDataControlOfferV1),
        Wlr(ZwlrDataControlOfferV1),
    }

    impl Offer {
        fn destroy(&self) {
            match self {
                Offer::Ext(offer) => offer.destroy(),
                Offer::Wlr(offer) => offer.destroy(),
            }
        }
    }

    struct DataControlState {
        sender: std::sync::mpsc::Sender<crate::platform::windows::ClipboardChange>,
        manager: Option<Manager>,
        seat: Option<wl_seat::WlSeat>,
        device: Option<DataControlDevice>,
        offers: Vec<Offer>,
        sequence: u32,
        roundtrip_done: bool,
    }

    impl Dispatch<wl_callback::WlCallback, ()> for DataControlState {
        fn event(
            state: &mut Self,
            _: &wl_callback::WlCallback,
            _: wl_callback::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            state.roundtrip_done = true;
        }
    }

    fn bounded_roundtrip(
        connection: &Connection,
        queue: &mut wayland_client::EventQueue<DataControlState>,
        state: &mut DataControlState,
        stop: i32,
    ) -> std::io::Result<()> {
        state.roundtrip_done = false;
        connection.display().sync(&queue.handle(), ());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !state.roundtrip_done {
            dispatch_once(queue, state, stop, Some(deadline))?;
            if std::time::Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
        }
        Ok(())
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for DataControlState {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name,
                interface,
                version,
            } = event
            {
                // Prefer the freedesktop ext manager; the wlr one is bound
                // only when no ext global has been advertised yet.
                match interface.as_str() {
                    "ext_data_control_manager_v1"
                        if !matches!(state.manager, Some(Manager::Ext(_))) =>
                    {
                        // bind() falls back to an inert proxy on error, which
                        // is fine: the global was advertised by the compositor.
                        let manager = registry.bind::<ExtDataControlManagerV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                        state.manager = Some(Manager::Ext(manager));
                    }
                    "zwlr_data_control_manager_v1" if state.manager.is_none() => {
                        let manager = registry.bind::<ZwlrDataControlManagerV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                        state.manager = Some(Manager::Wlr(manager));
                    }
                    "wl_seat" if state.seat.is_none() => {
                        let seat =
                            registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(1), qh, ());
                        state.seat = Some(seat);
                    }
                    _ => {}
                }
            }
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for DataControlState {
        fn event(
            _: &mut Self,
            _: &wl_seat::WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtDataControlManagerV1, ()> for DataControlState {
        fn event(
            _: &mut Self,
            _: &ExtDataControlManagerV1,
            _: ext_data_control_manager_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwlrDataControlManagerV1, ()> for DataControlState {
        fn event(
            _: &mut Self,
            _: &ZwlrDataControlManagerV1,
            _: zwlr_data_control_manager_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtDataControlDeviceV1, ()> for DataControlState {
        fn event(
            state: &mut Self,
            _: &ExtDataControlDeviceV1,
            event: ext_data_control_device_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            handle_device_event(state, event.into());
        }
    }

    impl Dispatch<ZwlrDataControlDeviceV1, ()> for DataControlState {
        fn event(
            state: &mut Self,
            _: &ZwlrDataControlDeviceV1,
            event: zwlr_data_control_device_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            handle_device_event(state, event.into());
        }
    }

    enum DeviceEvent {
        DataOffer(Offer),
        Selection(Option<Offer>),
        Other,
    }

    impl From<ext_data_control_device_v1::Event> for DeviceEvent {
        fn from(event: ext_data_control_device_v1::Event) -> Self {
            match event {
                ext_data_control_device_v1::Event::DataOffer { id } => {
                    DeviceEvent::DataOffer(Offer::Ext(id))
                }
                ext_data_control_device_v1::Event::Selection { id } => {
                    DeviceEvent::Selection(id.map(Offer::Ext))
                }
                _ => DeviceEvent::Other,
            }
        }
    }

    impl From<zwlr_data_control_device_v1::Event> for DeviceEvent {
        fn from(event: zwlr_data_control_device_v1::Event) -> Self {
            match event {
                zwlr_data_control_device_v1::Event::DataOffer { id } => {
                    DeviceEvent::DataOffer(Offer::Wlr(id))
                }
                zwlr_data_control_device_v1::Event::Selection { id } => {
                    DeviceEvent::Selection(id.map(Offer::Wlr))
                }
                _ => DeviceEvent::Other,
            }
        }
    }

    fn handle_device_event(state: &mut DataControlState, event: DeviceEvent) {
        match event {
            DeviceEvent::DataOffer(offer) => {
                for old in state.offers.drain(..) {
                    old.destroy();
                }
                state.offers.push(offer);
            }
            DeviceEvent::Selection(_) => {
                // The compositor sends data_offer + selection together for
                // every new CLIPBOARD owner; that is the change signal.
                state.sequence = state.sequence.wrapping_add(1);
                let _ = state
                    .sender
                    .send(crate::platform::windows::ClipboardChange {
                        sequence: state.sequence,
                    });
            }
            // Primary selection is not captured by the app; `finished` only
            // means the device went inert (compositor shutdown).
            DeviceEvent::Other => {}
        }
    }

    // --- Connection setup (runs on the calling thread) ---------------------
    let connection = Connection::connect_to_env().ok()?;
    let display = connection.display();
    let mut event_queue = connection.new_event_queue();
    let qh = event_queue.handle();
    let _registry = display.get_registry(&qh, ());
    let mut state = DataControlState {
        sender,
        manager: None,
        seat: None,
        device: None,
        offers: Vec::new(),
        sequence: 0,
        roundtrip_done: false,
    };
    // Collect the initial globals.
    if bounded_roundtrip(&connection, &mut event_queue, &mut state, stop.reader_fd()).is_err() {
        return None;
    }
    let (manager, seat) = match (state.manager.take(), state.seat.take()) {
        (Some(manager), Some(seat)) => (manager, seat),
        // Neither manager global (old GNOME) or no seat yet: fall back.
        _ => return None,
    };
    let device = manager.get_data_device(&seat, &qh);
    state.device = Some(device);
    // Flush the get_data_device request before the thread takes over.
    if bounded_roundtrip(&connection, &mut event_queue, &mut state, stop.reader_fd()).is_err() {
        return None;
    }

    let stop_reader_fd = stop.reader_fd();
    let spawn_result = std::thread::Builder::new()
        .name("wayland-clipboard-monitor".to_owned())
        .spawn(move || {
            while dispatch_once(&mut event_queue, &mut state, stop_reader_fd, None).is_ok() {}
            if let Some(device) = state.device.take() {
                device.destroy();
            }
            // SAFETY: the monitor thread owns the read end of the stop pipe.
            unsafe { libc::close(stop_reader_fd) };
        });
    match spawn_result {
        Ok(handle) => Some((handle, stop.into_writer())),
        // The pair's Drop closes both pipe fds; the connection is dropped
        // here without ever leaving this thread.
        Err(_) => None,
    }
}
