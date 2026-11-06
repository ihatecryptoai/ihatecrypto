//! Keeping Windows from idle sleep while a TWAP of ours runs, so the engine's guardian sees a stop that fired (the Mac's
//! ProcessInfo.beginActivity(.idleSystemSleepDisabled)). The display may still turn off; a closed lid or a quit app end it.

use windows::Win32::System::Power::{ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState};

/// On: the calling thread holds the system awake until it calls this with false, or ends. False when Windows refused.
pub fn stay_awake(on: bool) -> bool {
    let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
    unsafe { SetThreadExecutionState(flags) }.0 != 0
}
