# Stupid alloc - what if memory allocation was annoying
Mostly a weird exercise in how much you can make a memory allocator suck balls.

This allocator will create and open files to use as the allocation's data, through a memory map. If you enable the `interactive` feature, it will even prompt you for a file name every time
the program allocates something! Cool!


## How to use it
don't

## No but really how does one use this
It's not on [crates.io](https://crates.io) (yet), so if you **really** want to use it, clone the repo and use it as a local dependency. Note that you must be using nightly Rust.

```toml
[dependencies]
stupidalloc = { path = "/where/the/repo/is/stupidalloc" }
```

Afterwards, you have a few options:
- You can use it as the global allocator of your program, but it may lead to wonkiness and weird stuff like allocating before `main()` is executed!

```rust
use stupidalloc::StupidAlloc;

#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

fn main() {
    // ...
}
```

- By using the [`allocator_api`](https://doc.rust-lang.org/beta/unstable-book/library-features/allocator-api.html) nightly feature, you can selectively
allocate single objects with this allocator:

```rust
// Requires nightly
#![feature(allocator_api)]

use stupidalloc::StupidAlloc;

fn main() {
    let normal_box = Box::new(1);

    let stupid_box = Box::new_in(1, StupidAlloc);
}
```

A cool usage is to stop the execution of your program (through your favourite `stdin` read) and then go look at the allocation files with a hex editor (might I recommend [Hexyl](https://github.com/sharkdp/hexyl)?)

To help you with that, the allocator exposes a few helper functions:
- `StupidAlloc.state()` returns a `HashMap` where the key is the address of the memory map (and so the address of the allocated object), and the value is a `PathBuf` to the associated file.
- `StupidAlloc` implements `fmt::Display`, so running `println!("{StupidAlloc}")` will print a lovely summary of all the allocations currently being tracked.
- `StupidAlloc.file_of(x)` will return the file associated to the linked object, if it exists. Obviously this only works with stuff allocated with the stupid allocator. An example of use:

```rust
// Still requires nightly
#![feature(allocator_api)]

use stupidalloc::StupidAlloc;

fn main() {
    let stupid_box = Box::new_in(1, StupidAlloc);

    // Since it's a Box<i32>, we need to pass &i32 to the function to get the 
    // address of where the integer is.
    let file = StupidAlloc.file_of(&*stupid_box).unwrap();

    // Go nuts with it!
}
```

Another cool usage is to be able to see how stuff is laid out in memory, without
having to use memory viewers or complicated GDB syntax!

For example, ever wanted to see how a `Vec<T>` is organised in memory?

```rust
use stupidalloc::StupidAlloc;

#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

fn main() {
    let boxed_vec = Box::new(vec![1, 2, 3]);

    println!("{}", StupidAlloc.file_of(&*boxed_vec).display());

    // Somehow pause execution
}
```

This program will print the path of the allocation file for the `Vec<T>` struct
(and not the allocation for the data of the `Vec`, because then we'd only see
the numbers 1, 2, 3!). Open it in a hex viewer, and you can try and guess what
each field is, and try to corroborate it with the [struct's definition](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html).
If your system allows you to (I know Windows can be a bit restrictive), try and 
modify the length and/or capacity fields and see what happens afterwards!

## Disclaimers
- If you don't have file picker / file dialog capabilities (minimal i3 installation, TTY-only, ...), `interactivity` won't work. 
- I only tested this on Windows and Linux. If it doesn't work on MacOS or any other OS, sorry. If it doesn't work for you on Windows or Linux: weird! Hit me up.
- If you mess with the memory files in any way you'll mess up with your program memory, but seeing as this is topologically the same as messing with `/proc/mem` I consider this a cool feature.
- I'm probably going to work on this *a little bit more* to add some quality-of-life features, but that's it. It's a shitpost, not a serious library.

## (old) Demo
https://github.com/shadyfennec/stupidalloc/assets/68575248/f2490dc1-8412-4450-9359-7387f79682ea
