use stupidalloc::StupidAlloc;

#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

fn main() {
    let v = Box::new(Vec::<u8>::with_capacity(1000));

    let file = StupidAlloc.file_of(&*v).unwrap();
    println!("{file:?}");
}
