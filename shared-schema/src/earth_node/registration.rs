use crate::earth_node::message::hour::{self};

pub struct ProofOfWork {
    hour: hour::SinceEpoch,
}

pub struct Registration {
    proof_of_work: ProofOfWork,
}
