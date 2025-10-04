fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/sky.capnp")
        .run()
        .expect("schema compiler command");
}
