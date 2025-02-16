//! A very stupid but maybe educational memory allocator.
//!
//! # Behaviour
//! This [`Allocator`] will create, open and use a file for every single allocation
//! performed through it. Obviously, doing this imples allocating stuff,
//! which is kind of problematic. So, as a fallback, this allocator uses
//! [`System`] when allocating during a memory allocation or de-allocation.
//!
//! # Usage example
//! Use the allocator for a few items while keeping the global normal allocator
//!
//! ```
//! #![feature(allocator_api)] // You need this for the `new_in` functions. Requires nightly.
//! use stupidalloc::StupidAlloc;
//!
//! let normal_box = Box::new(1u32);
//!
//! let stupid_box = Box::new_in(1u32, StupidAlloc);
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
//! ## Graphics
//! Enabling the `graphics` feature will allow you to create interactive graphical
//! windows that will visually show the contents of the memory you allocate with
//! this allocator. The data will be represented as rows of bytes, that are themselves
//! represented as consecutive 8 bits. Graphically, each bit is shown as a black
//! or white square, where black represents a `0`, and white represents a `1`.
//!
//! ### Modifying memory contents with the mouse
//! Clicking with the left mouse button on a square will set the corresponding bit
//! in memory, while using the right mouse button will clear it. You can thus
//! directly modify memory using only your mouse, and directly see the results!
//!
//! ### Creation
//! By default, allocating memory doesn't create a corresponding graphical window.
//! To create a window, you can use `StupidAlloc::open_window_of()`, and you
//! can adjust the number of bytes displayed on each row using
//! `StupidAlloc::set_columns_of`:
//!
//! ```no_run
//! #![feature(allocator_api)] // You need this for the `new_in` functions. Requires nightly.
//! use stupidalloc::StupidAlloc;
//!
//! let stupid_box = Box::new_in(1u32, StupidAlloc);
//!
//! #[cfg(feature = "graphics")]
//! {
//!     // Start with 8 columns.
//!     StupidAlloc.open_window_of(&*stupid_box, 8);
//!
//!     // I changed my mind, I want 4 columns instead.
//!     StupidAlloc.set_columns_of(&*stupid_box, 4);
//! }
//! ```
//!
//! If the `always-graphics` feature is enabled, then every allocation will be
//! displayed automatically, without the need to call `open_window_of()`.
//!
//! ## Logging
//! If the `logging` feature is enabled, each allocation will be accompanied by
//! a companion log file, with the same path and name as the allocation file, but
//! with a `.md` extension. Inside, information about the allocation will be
//! written as the allocation is interacted with:
//! - Metadata, such as corresponding allocation file, the [`Layout`], ...
//! - Allocation and deallocation backtraces (requires the `RUST_BACKTRACE`
//!   environment variable to be set accordingly)
//! - Every grow or shrink, with new [`Layout`] and corresponding backtrace
//!
//! Log files won't be deleted when the corresponding memory is freed, but they
//! might get overwritten, either by you when using the `interactive` feature
//! and specifying the same file name as a previous allocation's, or by
//! subsequent executions of a program that uses this allocator.
//!
//! ## Multi-threading
//! Internally, the allocator uses a [`RwLock`] when allocating and de-allocating.
//! As such, using this in a multi-threaded context will yield even more awful
//! performance. Performance is not the goal, but be warned nonetheless.

#![feature(allocator_api)]
#![feature(ptr_metadata)]
#![feature(doc_cfg)]
#![warn(missing_docs)]

use core::fmt;
use hashbrown::{hash_map::DefaultHashBuilder, HashMap};
use lazy_static::lazy_static;
use memmap2::{MmapMut, MmapOptions};
use std::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout, System},
    fs::{File, OpenOptions},
    path::PathBuf,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Once, RwLock,
    },
};

#[cfg(feature = "interactive")]
use native_dialog::{FileDialog, MessageDialog, MessageType};

#[cfg(feature = "logging")]
use std::{backtrace::Backtrace, io::Write};

#[cfg(feature = "graphics")]
mod graphics;

// tuples are so 2016 let's use a struct instead
struct AllocHandle {
    // memory map of the data
    map: MmapMut,
    // we use an arc so that we can share the handle with the graphical display
    // thread.
    file: Arc<File, System>,
    // the path to the data-holding file.
    path: PathBuf,
    // the thread handle to the graphics thread, if enabled
    #[cfg(feature = "graphics")]
    window: Option<graphics::Window>,
    // the file handle of the logging file
    #[cfg(feature = "logging")]
    log_file: File,
}

lazy_static! {
    // use hashbrown map explicitly so that we can directly specify that it lives in
    // system allocator.
    static ref STUPID_MAP: RwLock<HashMap<usize, AllocHandle, DefaultHashBuilder, allocator_api2::alloc::System>> =
        RwLock::new(HashMap::new_in(allocator_api2::alloc::System));
}

// these are thread_local because they must not interfere with other threads.
thread_local! {
    // currently allocating? nonzero = yes.
    static ALLOCATING: AtomicUsize = AtomicUsize::new(0);
    // currently de-allocating? nonzero = yes.
    static DEALLOCATING: AtomicUsize = AtomicUsize::new(0);
    // thread-local inhibition boolean, true = use system.
    static LOCAL_SWITCH_OFF: AtomicBool = {
        // if init was completed, current thread is not main thread, disabling
        // by default. were it not for that, when using `always-graphics`, thread
        // internals would get allocated in recursion and that's the only viable
        // solution.
        if INIT_DETECTOR.is_completed() {
            AtomicBool::new(true)
        } else {
            // the init once was not called, so this is main thread (or more
            // generally the first thread that tries to use stupid alloc). allowing
            // stupid alloc by default.
            INIT_DETECTOR.call_once(|| {});
            AtomicBool::new(false)
        }
    }
}

// if this Once is not initialized, this means we are between program entry point
// and the first access to LOCAL_SWITCH_OFF (aka first stupid allocation).
static INIT_DETECTOR: Once = Once::new();

// the number of byte columns used by default when opening a window for a new
// allocation. default to 8 bytes (64 bits) per line.
#[cfg(feature = "always-graphics")]
static DEFAULT_GRAPHICS_COLUMNS: AtomicUsize = AtomicUsize::new(8);

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
        let path = std::env::temp_dir().join("stupidalloc"); // let's just say only one stupidalloc exists huh :)
        match std::fs::create_dir(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("stupidalloc temp dir creation failed: {e}"),
        };

        Some(path.join(format!(
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
    /// Enables or disables stupid allocation in the current thread, depending
    /// on the value passed as parameter. Passing `true` enables it, and `false`
    /// disables it.
    pub fn enable_in_thread(&self, value: bool) {
        LOCAL_SWITCH_OFF.with(|l| l.store(!value, Ordering::SeqCst));
    }

    /// Return a [`HashMap`] where the key is an address of an allocation and
    /// the value is a [`PathBuf`].
    pub fn state(&self) -> HashMap<usize, PathBuf> {
        STUPID_MAP
            .read()
            .unwrap()
            .iter()
            .map(|(&addr, handle)| (addr, handle.path.clone()))
            .collect()
    }

    /// Returns the [`PathBuf`] of the allocation of an element if it has been
    /// allocated with the stupid alloc.
    pub fn file_of<T: ?Sized>(&self, value: &T) -> Option<PathBuf> {
        STUPID_MAP
            .read()
            .unwrap()
            .iter()
            .find_map(|(&addr, handle)| {
                if (addr..addr + handle.map.len())
                    .contains(&(value as *const T as *const u8 as usize))
                {
                    Some(handle.path.clone())
                } else {
                    None
                }
            })
    }

    /// Opens a graphical window displaying the memory contents of the data
    /// passed as a parameter, if it was allocated with stupid alloc. You must also
    /// specify the number of bytes displayed on each row using the `columns`
    /// parameter.
    #[cfg(feature = "graphics")]
    #[doc(cfg(feature = "graphics"))]
    pub fn open_window_of<T: ?Sized>(&self, value: &T, columns: usize) {
        STUPID_MAP
            .write()
            .unwrap()
            .iter_mut()
            .for_each(|(&addr, handle)| {
                if (addr..addr + handle.map.len())
                    .contains(&(value as *const T as *const u8 as usize))
                {
                    if let Some(window) = handle.window.as_mut() {
                        if window.is_finished() {
                            *window = graphics::Window::new(
                                &handle.path,
                                Arc::clone(&handle.file),
                                columns,
                            );
                        }
                    } else {
                        handle.window = Some(graphics::Window::new(
                            &handle.path,
                            Arc::clone(&handle.file),
                            columns,
                        ));
                    }
                }
            })
    }

    /// If a graphical window is currently open for `value`, this sets its
    /// number of columns: the number of bytes (or groups of 8 bits) on each row.
    #[cfg(feature = "graphics")]
    #[doc(cfg(feature = "graphics"))]
    pub fn set_columns_of<T: ?Sized>(&self, value: &T, columns: usize) {
        STUPID_MAP
            .write()
            .unwrap()
            .iter_mut()
            .for_each(|(&addr, handle)| {
                if (addr..addr + handle.map.len())
                    .contains(&(value as *const T as *const u8 as usize))
                {
                    if let Some(window) = handle.window.as_mut() {
                        window
                            .tx
                            .send(graphics::Message::Resize { columns })
                            .unwrap();
                    }
                }
            })
    }

    /// Closes any graphical window associated with `value`.
    #[cfg(feature = "graphics")]
    pub fn close_graphics_of<T: ?Sized>(&self, value: &T) {
        STUPID_MAP
            .write()
            .unwrap()
            .iter_mut()
            .for_each(|(&addr, handle)| {
                if (addr..addr + handle.map.len())
                    .contains(&(value as *const T as *const u8 as usize))
                {
                    if let Some(window) = handle.window.take() {
                        window.close()
                    }
                }
            })
    }

    // this function abstracts Allocator::allocate and Allocator::allocate_zeroed
    // since the only way to allocate memory with stupid alloc is to have the
    // contents zeroed already. in the spirit of not duplicating code, the
    // fallback (either System::allocate or System::allocate_zeroed) is passed
    // as a parameter.
    fn inner_allocate<F>(&self, layout: Layout, fallback: F) -> Result<NonNull<[u8]>, AllocError>
    where
        F: Fn(Layout) -> Result<NonNull<[u8]>, AllocError>,
    {
        // we only allocate if
        // - we're allowed to
        // - we're not currently allocating with stupid alloc
        // - we're not currently de-allocating something from stupid alloc
        if LOCAL_SWITCH_OFF.with(|l| l.load(Ordering::SeqCst))
            || DEALLOCATING.with(|d| d.load(Ordering::SeqCst)) != 0
            || ALLOCATING.with(|a| a.load(Ordering::SeqCst)) != 0
        {
            // THIS IS STUPIDALLOC BITCH!!! we clown in this muthafucka betta
            // take yo sensitive ass back to System
            fallback(layout)
        } else {
            // okay so first we tell the thread that we're allocating.
            // no recursive allocation allowed this bricked my PC twice already.
            ALLOCATING.with(|a| a.fetch_add(1, Ordering::SeqCst));
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
                        let mut map = unsafe { MmapOptions::new().map_mut(&file).unwrap() };

                        let ptr = NonNull::from_raw_parts(
                            NonNull::new(map.as_mut_ptr() as *mut ()).unwrap(),
                            layout.size(),
                        );

                        // do some logging if we're told to
                        #[cfg(feature = "logging")]
                        let log_file = {
                            let mut log_path = path.clone();
                            log_path.set_extension("md");

                            let mut log_file = OpenOptions::new()
                                .read(true)
                                .write(true)
                                .truncate(true)
                                .create(true)
                                .open(log_path)
                                .unwrap();

                            writeln!(
                                log_file,
                                "# Metadata\n- Allocation path: {}\n- Layout: {layout:?}\n\n# Allocation\n```\n{}\n```\n\n# Events\n",
                                path.to_string_lossy(),
                                Backtrace::capture()
                            )
                            .unwrap();

                            log_file
                        };

                        // it's probably not necessary to specify System for
                        // this arc, but better be safe.
                        let file = Arc::new_in(file, System);

                        // we have graphics? decide if we start with a window
                        // for this alloc.
                        #[cfg(feature = "graphics")]
                        let window = {
                            // the feature is enabled: go wild!
                            #[cfg(feature = "always-graphics")]
                            {
                                Some(graphics::Window::new(
                                    &path,
                                    Arc::clone(&file),
                                    DEFAULT_GRAPHICS_COLUMNS.load(Ordering::SeqCst),
                                ))
                            }
                            // or not: no
                            #[cfg(not(feature = "always-graphics"))]
                            {
                                None
                            }
                        };

                        STUPID_MAP.write().unwrap().insert(
                            ptr.as_ptr() as *mut u8 as usize,
                            AllocHandle {
                                file,
                                map,
                                path,
                                #[cfg(feature = "graphics")]
                                window,
                                #[cfg(feature = "logging")]
                                log_file,
                            },
                        );

                        Ok(ptr)
                    } else {
                        Err(AllocError)
                    }
                } else {
                    Err(AllocError)
                }
            };

            // okay finally tell the thread we finished this allocation. if it's
            // back to zero we can potentially stupid alloc again!
            ALLOCATING.with(|a| a.fetch_sub(1, Ordering::SeqCst));

            result
        }
    }

    // like inner_allocate, this abstracts over grow, shrink and grow_zeroed,
    // since the implementation is the same for all of them, except which
    // function to use as a fallback.
    unsafe fn grow_or_shrink<F>(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
        fallback: F,
    ) -> Result<NonNull<[u8]>, AllocError>
    where
        F: Fn(NonNull<u8>, Layout, Layout) -> Result<NonNull<[u8]>, AllocError>,
    {
        let addr: usize = ptr.as_ptr() as usize;

        // same as allocate; if any of these is nonzero / true, we're guaranteed
        // the data was allocated by system.
        if LOCAL_SWITCH_OFF.with(|l| l.load(Ordering::SeqCst))
            || DEALLOCATING.with(|d| d.load(Ordering::SeqCst)) != 0
            || ALLOCATING.with(|a| a.load(Ordering::SeqCst)) != 0
        {
            fallback(ptr, old_layout, new_layout)
        } else if STUPID_MAP.read().unwrap().contains_key(&addr) {
            let handle = STUPID_MAP.write().unwrap().remove(&addr).unwrap();

            // grow or shrink, and growing zeroes stuff out.
            handle.file.set_len(new_layout.size() as u64).unwrap();

            // new memory mapping to reflect new size.
            let mut map = unsafe {
                MmapOptions::new()
                    .map_mut(&handle.file as &File /* thanks, memmap2 (sarcasm) */)
                    .unwrap()
            };

            // tell the window the size has changed
            #[cfg(feature = "graphics")]
            let window = {
                let mut window = handle.window;
                if let Some(window) = window.as_mut() {
                    window.tx.send(graphics::Message::Grow).unwrap();
                }
                window
            };

            // log the event
            #[cfg(feature = "logging")]
            let log_file = {
                let mut log_file = handle.log_file;
                writeln!(
                    log_file,
                    "## Resize\nNew layout: {new_layout:?}\n```\n{}\n```\n",
                    Backtrace::capture()
                )
                .unwrap();
                log_file
            };

            let ptr = NonNull::from_raw_parts(
                NonNull::new(map.as_mut_ptr() as *mut ()).unwrap(),
                new_layout.size(),
            );

            STUPID_MAP.write().unwrap().insert(
                ptr.as_ptr() as *mut u8 as usize,
                AllocHandle {
                    file: handle.file,
                    map,
                    path: handle.path,
                    #[cfg(feature = "graphics")]
                    window,
                    #[cfg(feature = "logging")]
                    log_file,
                },
            );

            Ok(ptr)
        } else {
            // this really shouldn't happen i think.
            unreachable!(
                "invariants specify stupid alloc resize, but pointer not in stupid alloc registry"
            )
        }
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
        self.inner_allocate(layout, |layout| System.allocate(layout))
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        self.grow_or_shrink(
            ptr,
            old_layout,
            new_layout,
            |ptr, old_layout, new_layout| System.grow(ptr, old_layout, new_layout),
        )
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        self.grow_or_shrink(
            ptr,
            old_layout,
            new_layout,
            |ptr, old_layout, new_layout| System.grow_zeroed(ptr, old_layout, new_layout),
        )
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() <= old_layout.size(),
            "`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
        );

        self.grow_or_shrink(
            ptr,
            old_layout,
            new_layout,
            |ptr, old_layout, new_layout| System.shrink(ptr, old_layout, new_layout),
        )
    }

    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.inner_allocate(layout, |layout| System.allocate_zeroed(layout))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let addr: usize = ptr.as_ptr() as usize;

        // same as allocate, if any of these is nonzero / true, the data was
        // allocated by system.
        if LOCAL_SWITCH_OFF.with(|l| l.load(Ordering::SeqCst))
            || DEALLOCATING.with(|d| d.load(Ordering::SeqCst)) != 0
            || ALLOCATING.with(|a| a.load(Ordering::SeqCst)) != 0
        {
            System.deallocate(ptr, layout);
        } else if STUPID_MAP.read().unwrap().contains_key(&addr) {
            // tell thread we're deallocating
            DEALLOCATING.with(|d| d.fetch_add(1, Ordering::SeqCst));

            // remove handle from map
            let handle = STUPID_MAP.write().unwrap().remove(&addr).unwrap();

            // log deallocation
            #[cfg(feature = "logging")]
            {
                let mut log_file = handle.log_file;
                writeln!(
                    log_file,
                    "# Deallocation\n```\n{}\n```",
                    Backtrace::capture()
                )
                .unwrap();
            }

            // close graphical window
            #[cfg(feature = "graphics")]
            {
                // if there is a window, we need to destroy that first
                if let Some(window) = handle.window {
                    window.tx.send(graphics::Message::Free).unwrap();
                    // originally i wanted to join the thread of the window
                    // because that's what good people do, but since de-allocation
                    // after main has ended means the threads were already killed,
                    // we run into a weird issue where join panics because
                    // its thread has been sweeped under itself and killed
                    // without its consent. so for now, until i find a good way
                    // of properly join a thread after main, let's just leave
                    // them be. they're all going to terminate because of the
                    // free message anyways.
                    //
                    // FIXME: find a way to join a thread even when it has been
                    //        killed by the end of process function.
                    //window.close();
                }
            }

            drop(handle.map); // the map needs to be dropped first
            drop(handle.file); // and then afterwards the file handle

            //std::thread::sleep(std::time::Duration::from_millis(1000));

            // this needs to be done during a time where DEALLOCATING is true,
            // since it allocates and you'd end up in an infinite recursion.
            std::fs::remove_file(handle.path).unwrap();

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

            // tell thread we're done deallocating
            DEALLOCATING.with(|a| a.fetch_sub(1, Ordering::SeqCst));
        } else {
            unreachable!("invariants specify stupid alloc deallocation, but data not present in stupid alloc registry")
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

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        <Self as Allocator>::allocate_zeroed(self, layout)
            .unwrap()
            .as_ptr() as _
    }
}
