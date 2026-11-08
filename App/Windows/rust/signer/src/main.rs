//! The signing process on Windows. Started with pipes on its standard input and output by the engine
//! (ihatecrypto-engine.exe, next to it), or — for an engine in its AppContainer, which may start no process — by the
//! window (ihatecrypto.exe, in the folder above), which hands the pipes to the engine; it takes requests only from
//! there. Frames: a 4-byte little-endian length, then that many bytes of JSON (SignerRequest in, SignerReply out). One
//! request at a time; the engine's pipe closing ends the signer.

use std::io::Write;

use signer::{host, platform};

fn main() {
    let failed = platform::harden();
    *host::NOTES.lock().expect("notes") = host::describe(failed);
    if !platform::parent_check("ihatecrypto-engine.exe") && !platform::parent_check_above("ihatecrypto.exe") {
        eprintln!("ihatecrypto signer: started by something other than ihatecrypto's engine; nothing is served.");
        std::process::exit(3);
    }
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    while let Some(request) = signer_api::read_frame(&mut input) {
        // An imported phrase comes in these bytes: wiped once served.
        let request = zeroize::Zeroizing::new(request);
        if signer_api::write_frame(&mut output, &host::handle(&request)).and_then(|_| output.flush()).is_err() { break }
    }
}
