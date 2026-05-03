# Discrete Event Network Simulator (DENS)

A discrete event simulator modeled similarly to [turmoil](https://crates.io/crates/turmoil). 

## Architecture

Overall Sim 

Uses one Tokio runtime for each machine in order to perform scheduling. We use one  Tokio's [Instant](https://docs.rs/tokio/latest/tokio/time/struct.Instant.html) will change based on the discrete event simulator's time rather than based on the OS's time.
