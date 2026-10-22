use std::{
    io::Read,
    os::unix::net::UnixStream,
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

use super::ServerState;

#[test]
fn stopping_before_registration_rejects_and_closes_the_stream() -> Result<(), String> {
    let state = ServerState::default();
    let (accepted, peer) = UnixStream::pair().map_err(|error| error.to_string())?;
    set_peer_read_timeout(&peer)?;
    state.request_stop();

    if state
        .register(&accepted)
        .map_err(|error| error.to_string())?
    {
        return Err("The stopped server registered a new stream.".to_owned());
    }
    if state.has_active_stream() {
        return Err("The stopped server retained an active stream.".to_owned());
    }

    assert_peer_closed(peer)
}

#[test]
fn a_stop_racing_with_registration_closes_the_stream() -> Result<(), String> {
    for _ in 0..16 {
        let state = Arc::new(ServerState::default());
        let (accepted, peer) = UnixStream::pair()
            .map_err(|error| format!("Could not create a stream pair: {error}."))?;
        set_peer_read_timeout(&peer)?;
        let barrier = Arc::new(Barrier::new(3));

        let registration_state = Arc::clone(&state);
        let registration_barrier = Arc::clone(&barrier);
        let registration = thread::spawn(move || {
            let _started = registration_barrier.wait();
            registration_state.register(&accepted)
        });

        let stopping_state = Arc::clone(&state);
        let stopping_barrier = Arc::clone(&barrier);
        let stopping = thread::spawn(move || {
            let _started = stopping_barrier.wait();
            stopping_state.request_stop();
        });

        let _started = barrier.wait();
        let _registered = registration
            .join()
            .map_err(|_| "The registration thread failed.".to_owned())?
            .map_err(|error| format!("Could not register the active stream: {error}."))?;
        stopping
            .join()
            .map_err(|_| "The stop thread failed.".to_owned())?;

        if !state.is_stopping() || state.has_active_stream() {
            return Err("The stop race left the server active.".to_owned());
        }
        assert_peer_closed(peer).map_err(|error| format!("The stop race: {error}"))?;
    }
    Ok(())
}

fn assert_peer_closed(mut peer: UnixStream) -> Result<(), String> {
    let mut byte = [0_u8; 1];
    match peer.read(&mut byte) {
        Ok(0) => Ok(()),
        Ok(read) => Err(format!("The cancelled stream returned {read} bytes.")),
        Err(error) => Err(format!("The peer read failed: {error}.")),
    }
}

fn set_peer_read_timeout(peer: &UnixStream) -> Result<(), String> {
    peer.set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| format!("Could not set peer read timeout: {error}."))
}
