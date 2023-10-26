use super::{Block, I3Block, I3Event};
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet, Operation};
use libpulse_binding::context::{self, introspect::SinkInfo};
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, thiserror::Error)]
enum PulseError {
    #[error("Failed to create mainloop")]
    NoMainloop,
    #[error("Iteration failed")]
    IterationFailed,
    #[error("Context terminated")]
    ContextTerminated,
}

enum PulseCommand {
    VolUp,
    VolDown,
    ToggleMute,
    QuitThread,
}

pub struct VolumeBlock {
    /// Cancels the usual interval timer when written to
    timer_cancel: Arc<Mutex<Sender<()>>>,
    /// Something to tell pulse what to do
    command_sender: Arc<Mutex<Sender<PulseCommand>>>,
    /// The state to display
    state: Arc<RwLock<Option<PulseState>>>,
}

impl Block for VolumeBlock {
    fn render(&self) -> Option<I3Block> {
        if let Some(state) = &*self.state.read().unwrap() {
            if state.muted {
                return Some(I3Block {
                    full_text: "muted".to_owned(),
                    color: Some("#ff0202".to_owned()),
                    ..Default::default()
                });
            }
            Some(I3Block {
                full_text: format!("{}%", state.volume),
                ..Default::default()
            })
        } else {
            None
        }
    }

    fn click(&self, evt: &I3Event) {
        match evt.button {
            1 => {
                std::thread::spawn(|| {
                    std::process::Command::new("pavucontrol")
                        .spawn()
                        .unwrap()
                        .wait()
                });
            }
            3 => {
                let _idc = self
                    .command_sender
                    .lock()
                    .unwrap()
                    .send(PulseCommand::ToggleMute);
            }
            4 => {
                let _idc = self
                    .command_sender
                    .lock()
                    .unwrap()
                    .send(PulseCommand::VolUp);
            }
            5 => {
                let _idc = self
                    .command_sender
                    .lock()
                    .unwrap()
                    .send(PulseCommand::VolDown);
            }
            _ => {}
        }
    }
}

struct PulseState {
    volume: u32,
    muted: bool,
}
enum PulseEvent {
    Changed(PulseState),
    Reconnect,
}

impl VolumeBlock {
    pub fn new(timer_cancel: Sender<()>) -> Self {
        let (cmd_sender, cmd_receiver) = std::sync::mpsc::channel();
        let ret = Self {
            timer_cancel: Arc::new(Mutex::new(timer_cancel)),
            state: Arc::new(RwLock::new(None)),
            command_sender: Arc::new(Mutex::new(cmd_sender)),
        };

        // Start Pulse thread
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender2 = sender.clone();
        let state2 = ret.state.clone();
        let cancel2 = ret.timer_cancel.clone();
        let cmd_sender2 = ret.command_sender.clone();
        let mut handle = std::thread::spawn(move || pulse_thread(sender2, cmd_receiver));
        std::thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(PulseEvent::Reconnect) => {
                        // Connection died, let's reconnect
                        let _idc = handle.join(); // Wait for the thread to die
                        let sender2 = sender.clone();
                        let _idc = cmd_sender2.lock().unwrap().send(PulseCommand::QuitThread); // Quit command
                                                                                               // thread
                        let (cmd_sender, cmd_receiver) = std::sync::mpsc::channel();
                        *cmd_sender2.lock().unwrap() = cmd_sender;
                        handle = std::thread::spawn(move || pulse_thread(sender2, cmd_receiver));
                    }
                    Ok(PulseEvent::Changed(state)) => {
                        *state2.write().unwrap() = Some(state);
                        let _idc = cancel2.lock().unwrap().send(());
                    }
                    Err(_) => {}
                }
            }
        });
        ret
    }
}

struct State {
    volume: u32,
    muted: bool,
    default_sink_index: Option<u32>,
    default_sink_name: Option<String>,
    raw_volume: Option<ChannelVolumes>,
}

#[allow(clippy::too_many_lines)]
fn pulse_thread(
    sender: Sender<PulseEvent>,
    receiver: Receiver<PulseCommand>,
) -> Result<(), PulseError> {
    // Initialize main loop
    let mainloop = Rc::new(RefCell::new(Mainloop::new().ok_or(PulseError::NoMainloop)?));

    // Initialize context
    let context = Arc::new(RwLock::new(
        Context::new(&*mainloop.borrow(), "statusbar-rs").expect("Failed to create new context"),
    ));

    // Prepare state
    let state = Arc::new(RwLock::new(State {
        volume: 0,
        muted: false,
        default_sink_index: None,
        default_sink_name: None,
        raw_volume: None,
    }));

    // Connect the context
    context
        .write()
        .unwrap()
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .expect("Failed to connect context");

    // Wait for context to be ready
    loop {
        match mainloop.borrow_mut().iterate(false) {
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(PulseError::IterationFailed);
            }
            IterateResult::Success(_) => {}
        }
        match context.read().unwrap().get_state() {
            libpulse_binding::context::State::Ready => {
                break;
            }
            libpulse_binding::context::State::Failed
            | libpulse_binding::context::State::Terminated => {
                return Err(PulseError::ContextTerminated);
            }
            _ => {}
        }
    }

    // Set subscribe callback
    let sender = Rc::new(sender);
    context
        .write()
        .unwrap()
        .set_subscribe_callback(Some(Box::new({
            let context = Arc::clone(&context);
            let state = Arc::clone(&state);
            let sender = Rc::clone(&sender);
            move |facility, operation, index| {
                // Did something about the default sink change?
                if facility == Some(Facility::Sink)
                    && operation == Some(Operation::Changed)
                    && Some(index) == state.read().unwrap().default_sink_index
                {
                    context
                        .read()
                        .unwrap()
                        .introspect()
                        .get_sink_info_by_index(index, {
                            let state = Arc::clone(&state);
                            let sender = Sender::clone(&sender);
                            move |sink_info| {
                                if let ListResult::Item(sink_info) = sink_info {
                                    parse_sink_info(
                                        sink_info,
                                        &mut state.write().unwrap(),
                                        &sender,
                                    );
                                }
                            }
                        });
                }
                // Did the default sink change?
                if facility == Some(Facility::Server) && operation == Some(Operation::Changed) {
                    request_server_info(&context, &state, &sender);
                }
            }
        })));

    // Subscribe to events
    let interest = InterestMaskSet::SERVER | InterestMaskSet::SINK;
    context.write().unwrap().subscribe(interest, |_| {});

    // Request initial server info
    request_server_info(&context, &state, &sender);

    // Handle commands
    let context2 = context.clone();
    std::thread::spawn(move || loop {
        let Ok(msg) = receiver.recv() else { continue; };
        let state = state.read().unwrap();
        match msg {
            PulseCommand::VolUp => {
                if let Some(sink) = state.default_sink_index {
                    let mut vol = state.raw_volume.unwrap();
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss
                    )]
                    vol.increase(Volume((5.0 * (Volume::NORMAL.0 as f32 / 100.0)) as u32));
                    context2
                        .read()
                        .unwrap()
                        .introspect()
                        .set_sink_volume_by_index(sink, &vol, None);
                }
            }
            PulseCommand::VolDown => {
                if let Some(sink) = state.default_sink_index {
                    let mut vol = state.raw_volume.unwrap();
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::cast_precision_loss
                    )]
                    vol.decrease(Volume((5.0 * (Volume::NORMAL.0 as f32 / 100.0)) as u32));
                    context2
                        .read()
                        .unwrap()
                        .introspect()
                        .set_sink_volume_by_index(sink, &vol, None);
                }
            }
            PulseCommand::ToggleMute => {
                if let Some(sink) = state.default_sink_index {
                    context2
                        .read()
                        .unwrap()
                        .introspect()
                        .set_sink_mute_by_index(sink, !state.muted, None);
                }
            }
            PulseCommand::QuitThread => {
                return;
            }
        };
    });

    // Main loop
    loop {
        match mainloop.borrow_mut().iterate(true) {
            IterateResult::Quit(_) => {
                eprintln!("Quit");
                break;
            }
            IterateResult::Err(_) => {
                eprintln!("iterate state was not success, quitting...");
                break;
            }
            IterateResult::Success(_) => {}
        }
        // Die if we disconnected
        if context.read().unwrap().get_state() != context::State::Ready {
            let _idc = sender.send(PulseEvent::Reconnect);
            break;
        }
    }

    Ok(())
}

/// Requests the server info and parses it into the passed state
fn request_server_info(
    context: &Arc<RwLock<Context>>,
    state: &Arc<RwLock<State>>,
    sender: &Rc<Sender<PulseEvent>>,
) {
    context.read().unwrap().introspect().get_server_info({
        let context = Arc::clone(context);
        let state = Arc::clone(state);
        let sender = Sender::clone(sender);
        move |info| {
            if let Some(name) = &info.default_sink_name {
                // Do nothing if the sink didnt change
                if info.default_sink_name.clone().map(|x| x.to_string())
                    != state.read().unwrap().default_sink_name
                {
                    // Request info for default sink
                    context
                        .read()
                        .unwrap()
                        .introspect()
                        .get_sink_info_by_name(name, {
                            let state = Arc::clone(&state);
                            let sender = Sender::clone(&sender);
                            move |sink_info| {
                                if let ListResult::Item(sink_info) = sink_info {
                                    parse_sink_info(
                                        sink_info,
                                        &mut state.write().unwrap(),
                                        &sender,
                                    );
                                }
                            }
                        });
                }
            }
        }
    });
}

/// Parses sink info into the state
fn parse_sink_info(info: &SinkInfo, state: &mut State, sender: &Sender<PulseEvent>) {
    state.default_sink_index = Some(info.index);
    state.default_sink_name = info.name.clone().map(|x| x.to_string());
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let volume = ((info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32) * 100.) as u32;
    let muted = info.mute;
    if volume != state.volume || muted != state.muted {
        state.volume = volume;
        state.muted = muted;
        state.raw_volume = Some(info.volume);
        let _idc = sender.send(PulseEvent::Changed(PulseState { volume, muted }));
    }
}
