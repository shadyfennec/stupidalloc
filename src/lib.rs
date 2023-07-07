//! A very stupid but maybe educational memory allocator.
//!
//! # Behaviour
//! This [`Allocator`] will create, open and use a file for every single allocation
//! performed through it. Obviously, doing this necessitates to allocate stuff,
//! which is kind of problematic. So, as a fallback, this allocator uses
//! [`System`] when allocating during a memory allocation or de-allocation.
//!
//! ## Interactivty
//! By default, the allocator will silently and automatically allocate memory
//! (as you would expect), by opening files in a temporary folder (as dictated
//! by [`std::env::temp_dir()`]). A feature flag, `interactive`, will enable
//! confirmation and file picking dialogs to pop up during allocations and
//! de-allocations. More specifically:
//! - On allocation, a confirmation message detailling the [`Layout`] needed for
//!   the allocation, followed by file picking dialog. If the first confirmation
//!   message is denied, or if no file is provided, the allocation fails.
//! - On de-allocation, a confirmation message showing the address of the thing
//!   that was de-allocated shows up. It doesn't matter how it is handled,
//!   the de-allocation won't fail because of it.
//!
//! # Usage example
//! Use the allocator for a few items while keeping the global normal allocator
//!
//! ```
//! #![feature(allocator_api)] // You need this for the `new_in` functions. Requires nightly.
//! use stupidalloc::StupidAlloc;
//!
//! let normal_box = Box::new(1);
//!
//! let stupid_box = Box::new_in(1, StupidAlloc);
//! ```
//!
//! Use the allocator as the global allocator. Warning: funky stuff may happen,
//! such as allocations before main!
//! ```
//! use stupidalloc::StupidAlloc;
//!
//! #[global_allocator]
//! static GLOBAL: StupidAlloc = StupidAlloc;
//!
//! fn main() {
//!     // ...
//! }
//! ```

#![feature(allocator_api)]
#![feature(ptr_metadata)]

#[cfg(feature = "interactive")]
use native_dialog::{FileDialog, MessageDialog, MessageType};

use core::fmt;
use lazy_static::lazy_static;
use memmap2::{MmapMut, MmapOptions};
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

// tuples are so 2016 let's use a struct instead
struct AllocHandle {
    map: MmapMut,
    file: File,
    path: PathBuf,
}

lazy_static! {
    // I use with_capacity with a big-ass capacity here because if the hashmap
    // has to resize itself it will lead to a deadlock of the mutex lol
    // don't have more than a million allocations at the same time i guess
    static ref STUPID_MAP: Mutex<HashMap<usize, AllocHandle>> =
        Mutex::new(HashMap::with_capacity(1_000_000));
}

// this is fetch_or'ed and it's the global switch between stupid and system
static ALLOCATING: AtomicBool = AtomicBool::new(false);
// in the dealloc especially, you need to use the system alloc, otherwise you
// run into nasty deadlock issues. so this flag is checked first when allocating
static DEALLOCATING: AtomicBool = AtomicBool::new(false);

// returns true if we do allocate something. only does something with the
// "interactive" feature enabled
#[allow(unused_variables)]
fn confirm_alloc(layout: Layout) -> bool {
    #[cfg(feature = "interactive")]
    {
        // show a lil' confirmation message before throwing you the
        // file chooser
        MessageDialog::new()
            .set_type(MessageType::Info)
            .set_title("Stupid allocation time!")
            .set_text(&format!(
                "Choose a file to allocate something for a layout of {layout:?}"
            ))
            .show_confirm()
            .unwrap()
    }

    #[cfg(not(feature = "interactive"))]
    {
        // if we're not interactive we don't ask the user if they want to
        // allocate stuff lol
        true
    }
}

// potentially returns a path to the file of the next allocation
fn get_alloc_file_path() -> Option<PathBuf> {
    #[cfg(feature = "interactive")]
    {
        // this is the file dialog thing
        FileDialog::new().show_save_single_file().unwrap()
    }
    #[cfg(not(feature = "interactive"))]
    {
        use std::sync::atomic::AtomicU64;

        // create a file with an increasing number for file name in the temp
        // folder.
        static ALLOC_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

        Some(std::env::temp_dir().join(format!(
            "alloc_{:010}.mem",
            ALLOC_FILE_COUNTER.fetch_add(1, Ordering::SeqCst)
        )))
    }
}

/// The stupid allocator.
///
/// See the [top-level documentation][crate] for more details.
pub struct StupidAlloc;

impl StupidAlloc {
    /// Return a [`HashMap`] where the key is an address of an allocation and
    /// the value is a [`PathBuf`].
    pub fn state(&self) -> HashMap<usize, PathBuf> {
        STUPID_MAP
            .lock()
            .unwrap()
            .iter()
            .map(
                |(
                    &addr,
                    AllocHandle {
                        map: _,
                        file: _,
                        path,
                    },
                )| (addr, path.clone()),
            )
            .collect()
    }

    /// Returns the [`PathBuf`] of the allocation of an element if it has been
    /// allocating with the stupid alloc.
    pub fn file_of<T: ?Sized>(&self, value: &T) -> Option<PathBuf> {
        STUPID_MAP
            .lock()
            .unwrap()
            .iter()
            .find_map(|(&addr, AllocHandle { map, file: _, path })| {
                if (addr..addr + map.len()).contains(&(value as *const T as *const u8 as usize)) {
                    Some(path.clone())
                } else {
                    None
                }
            })
    }
}

impl fmt::Display for StupidAlloc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Stupid allocation state:")?;
        self.state().into_iter().try_for_each(|(addr, path)| {
            writeln!(f, "- 0x{addr:08x} @ {}", path.to_string_lossy())
        })?;
        Ok(())
    }
}

unsafe impl Allocator for StupidAlloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if DEALLOCATING.load(Ordering::SeqCst)
            || ALLOCATING.fetch_or(true, Ordering::SeqCst)
            || STUPID_MAP.try_lock().is_err()
        {
            // we're gonna allocate lots of memory for the stupid allocation,
            // and well we can't really use the stupid allocator to allocate
            // memory for the stupid allocator now can we? so fall-back to the
            // normal allocator when needed.
            System.allocate(layout)
        } else {
            let result = {
                if confirm_alloc(layout) {
                    let path = get_alloc_file_path();

                    if let Some(path) = path {
                        let file = OpenOptions::new()
                            .read(true)
                            .write(true)
                            .truncate(true)
                            .create(true)
                            .open(&path)
                            .unwrap();

                        file.set_len(layout.size() as u64).unwrap();
                        let map = unsafe { MmapOptions::new().map_mut(&file).unwrap() };

                        let ptr = NonNull::from_raw_parts(
                            NonNull::new(map.as_ptr() as _).unwrap(),
                            layout.size(),
                        );

                        STUPID_MAP.lock().unwrap().insert(
                            ptr.as_ptr() as *mut u8 as usize,
                            AllocHandle { file, map, path },
                        );

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
        if let Some(AllocHandle { map, file, path }) = lock.remove(&addr) {
            drop(lock); // first we need to drop the lock, otherwise we deadlock
            drop(map); // the map needs to be dropped first
            drop(file); // and then afterwards the file handle

            DEALLOCATING.store(true, Ordering::SeqCst);

            // this needs to be done during a time where DEALLOCATING is true,
            // since it allocates and you'd end up in an infinite recursion.
            std::fs::remove_file(path).unwrap();

            // show a lil confirmation message box
            #[cfg(feature = "interactive")]
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
