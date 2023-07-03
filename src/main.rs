#![feature(allocator_api)]
#![feature(ptr_metadata)]

use lazy_static::lazy_static;
use memmap2::{MmapMut, MmapOptions};
use native_dialog::{FileDialog, MessageDialog, MessageType};
use std::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout, System},
    collections::HashMap,
    fs::{File, OpenOptions},
    path::PathBuf,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

lazy_static! {
    // I use with_capacity with a big-ass capacity here because if the hashmap
    // has to resize itself it will lead to a deadlock of the mutex lol
    // don't have more than a million allocations at the same time i guess
    pub static ref STUPID_MAP: Mutex<HashMap<usize, (MmapMut, File, PathBuf)>> =
        Mutex::new(HashMap::with_capacity(1_000_000));
}

// this is fetch_or'ed and it's the global switch between stupid and system
static ALLOCATING: AtomicBool = AtomicBool::new(false);
// in the dealloc especially, you need to use the system alloc, otherwise you
// run into nasty deadlock issues. so this flag is checked first when allocating
static DEALLOCATING: AtomicBool = AtomicBool::new(false);

pub struct StupidAlloc;

unsafe impl Allocator for StupidAlloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if DEALLOCATING.load(Ordering::SeqCst) || ALLOCATING.fetch_or(true, Ordering::SeqCst) {
            // we're gonna allocate lots of memory for the stupid allocation,
            // and well we can't really use the stupid allocator to allocate
            // memory for the stupid allocator now can we? so fall-back to the
            // normal allocator when needed.
            System.allocate(layout)
        } else {
            let result = {
                // show a lil' confirmation message before throwing you the
                // file chooser
                let yes = MessageDialog::new()
                    .set_type(MessageType::Info)
                    .set_title("Stupid allocation time!")
                    .set_text(&format!(
                        "Choose a file to allocate something for a layout of {layout:?}"
                    ))
                    .show_confirm()
                    .unwrap();

                if yes {
                    // this is the file dialog thing
                    let path = FileDialog::new().show_save_single_file().unwrap();

                    if let Some(path) = path {
                        let file = OpenOptions::new()
                            .read(true)
                            .write(true)
                            .truncate(true)
                            .create(true)
                            .open(&path)
                            .unwrap();

                        file.set_len(layout.size() as u64).unwrap();
                        let mmap = unsafe { MmapOptions::new().map_mut(&file).unwrap() };

                        let ptr = NonNull::from_raw_parts(
                            NonNull::new(mmap.as_ptr() as _).unwrap(),
                            layout.size(),
                        );

                        STUPID_MAP
                            .lock()
                            .unwrap()
                            .insert(ptr.as_ptr() as *mut u8 as usize, (mmap, file, path));

                        Ok(ptr)
                    } else {
                        Err(AllocError)
                    }
                } else {
                    Err(AllocError)
                }
            };
            ALLOCATING.store(false, Ordering::SeqCst);

            result
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let addr: usize = ptr.as_ptr() as *mut u8 as usize;

        let mut lock = STUPID_MAP.lock().unwrap();

        // if the pointer we're asked to free isn't in our hashmap, then it's
        // obviously something allocated through the normal allocator.
        if let Some((_map, _file, path)) = lock.remove(&addr) {
            drop(lock); // first we need to drop the lock, otherwise we deadlock
            drop(_map); // the map needs to be dropped first
            drop(_file); // and then afterwards the file handle

            DEALLOCATING.store(true, Ordering::SeqCst);

            // this needs to be done during a time where DEALLOCATING is true,
            // since it allocates and you'd end up in an infinite recursion.
            std::fs::remove_file(path).unwrap();

            // show a lil confirmation message box
            let _ = MessageDialog::new()
                .set_type(MessageType::Info)
                .set_title("Stupid deallocation done!")
                .set_text(&format!(
                    "Allocation of layout {layout:?} at address 0x{addr:08x} free'd!"
                ))
                .show_confirm()
                .unwrap();

            DEALLOCATING.store(false, Ordering::SeqCst);
        } else {
            System.deallocate(ptr, layout);
        }
    }
}

unsafe impl GlobalAlloc for StupidAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        <Self as Allocator>::allocate(self, layout)
            .unwrap()
            .as_ptr() as _
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        <Self as Allocator>::deallocate(self, NonNull::new(ptr as _).unwrap(), layout)
    }
}

#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

fn main() {}
