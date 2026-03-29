use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn get_system_time() -> SystemTime {
    #[cfg(test)]
    let curr_time = {
        use end_to_end_test::{OsShim, sim::Sim};

        if Sim::is_in_machine_type::<OsShim>() {
            use end_to_end_test::Machine;
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

pub fn since_epoch() -> Duration {
    get_system_time().duration_since(UNIX_EPOCH).unwrap()
}
