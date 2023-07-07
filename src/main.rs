use stupidalloc::StupidAlloc;

#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

fn main() {
    let boxed_vec = Box::new(vec![1, 2, 3]);

    println!("{}", StupidAlloc.file_of(&*boxed_vec).unwrap().display());

    // Somehow pause execution
}
