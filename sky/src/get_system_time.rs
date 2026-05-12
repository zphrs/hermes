use std::time::SystemTime;
#[cfg(test)]
use std::time::{Duration, UNIX_EPOCH};

pub fn get_system_time() -> SystemTime {
    #[cfg(test)]
    let curr_time = {
        use dens::{OsShim, sim::Sim};

        if Sim::is_in_machine_type::<OsShim>() {
            use dens::Machine;
            use tracing::debug;

            let shim = Sim::get_current_machine::<OsShim>();
            let systime = shim.borrow().basic_machine().sys_time();
            debug!(?systime);
            systime
        } else {
            panic!()
        }
    };
    #[cfg(not(test))]
    let curr_time = {
        use std::time::SystemTime;
        SystemTime::now()
    };
    curr_time
}

#[cfg(test)]
pub fn since_epoch() -> Duration {
    get_system_time().duration_since(UNIX_EPOCH).unwrap()
}
