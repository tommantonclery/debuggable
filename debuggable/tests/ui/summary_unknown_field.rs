#[derive(debuggable::Debuggable)]
#[debuggable(summary = "{lenght} items")]
struct Stack {
    items: Vec<u8>,
    length: usize,
}

fn main() {}
