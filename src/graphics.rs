use std::{
    alloc::System,
    fs::File,
    path::Path,
    sync::{
        mpsc::{channel, Sender, TryRecvError},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use memmap2::{MmapMut, MmapOptions};
use minifb::{Scale, WindowOptions};

// iterator over bits of byte (LSB -> MSB)
fn bits_as_pixels(byte: u8) -> impl Iterator<Item = u32> {
    let byte = byte.reverse_bits();

    (0..8).map(move |i| {
        // minifb pixel format is 0x00RRGGBB
        if byte >> i & 1 == 0 {
            //00RRGGBB
            0x00000000
        } else {
            //00RRGGBB
            0x00FFFFFF
        }
    })
}

// code deduplication ugly function
fn create_map_window_buffer(
    file: &File,
    name: &str,
    columns: usize,
) -> (MmapMut, minifb::Window, Vec<u32, System>) {
    let map = unsafe { MmapOptions::new().map_mut(file).unwrap() };
    let mut window = minifb::Window::new(
        name,
        8 * columns,
        map.len() / columns,
        WindowOptions {
            scale: Scale::X16, // so that bits aren't the size of a pixel of your screen
            ..Default::default()
        },
    )
    .unwrap();
    window.limit_update_rate(Some(Duration::from_millis(16))); // 60 fps 😎

    let buffer = Vec::with_capacity_in(map.len() * 8, System);

    (map, window, buffer)
}

// messages sent by the allocator
pub enum Message {
    // grow (or shrink or grow_zeroed actually)
    Grow,
    // dealloc
    Free,
    // new column size
    Resize { columns: usize },
}

pub struct Window {
    // it's an option so that drop can join the thread by `take()`-ing it
    pub handle: Option<JoinHandle<()>>,
    pub tx: Sender<Message>,
}

impl Window {
    pub fn new(path: &Path, file: Arc<File, System>, columns: usize) -> Self {
        let name = format!("Graphical view of memory @ {}", path.to_string_lossy());

        let (tx, rx) = channel::<Message>();

        let handle = std::thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                let file = file;
                let mut columns = columns;

                let (mut map, mut window, mut buffer) =
                    create_map_window_buffer(&file, &name, columns);

                loop {
                    if !window.is_open() {
                        break;
                    }

                    match rx.try_recv() {
                        Err(TryRecvError::Empty) => {}
                        Ok(Message::Free) | Err(TryRecvError::Disconnected) => {
                            break;
                        }
                        Ok(Message::Grow) => {
                            let (new_map, new_window, new_buffer) =
                                create_map_window_buffer(&file, &name, columns);
                            map = new_map;
                            window = new_window;
                            buffer = new_buffer;
                        }
                        Ok(Message::Resize { columns: c }) => {
                            columns = c;
                            let (new_map, new_window, new_buffer) =
                                create_map_window_buffer(&file, &name, columns);
                            map = new_map;
                            window = new_window;
                            buffer = new_buffer;
                        }
                    }

                    // really proud of these two lines
                    buffer.clear();
                    buffer.extend(map.iter().flat_map(|b| bits_as_pixels(*b)));

                    window
                        .update_with_buffer(&buffer, 8 * columns, map.len() / columns)
                        .unwrap();

                    // i've been writing this feature for like 9 hours i'm too tired to try and de-duplicate this code
                    // future me or anyone else you're welcome to but i'd rather go to bed than try and do that
                    if window.get_mouse_down(minifb::MouseButton::Left) {
                        // set bit
                        if let Some((x, y)) = window.get_mouse_pos(minifb::MouseMode::Discard) {
                            let x = x.floor() as usize;
                            let y = y.floor() as usize;

                            let bit = x % 8;
                            let byte = (x / 8) + (y * columns);

                            let mask = 1 << (7 - bit);

                            map[byte] |= mask;
                        }
                    } else if window.get_mouse_down(minifb::MouseButton::Right) {
                        // clear bit
                        if let Some((x, y)) = window.get_mouse_pos(minifb::MouseMode::Discard) {
                            let x = x.floor() as usize;
                            let y = y.floor() as usize;

                            let bit = x % 8;
                            let byte = (x / 8) + (y * columns);

                            let mask = 1 << (7 - bit);

                            map[byte] &= !mask;
                        }
                    }
                }
            })
            .unwrap();

        Window {
            handle: Some(handle),
            tx,
        }
    }

    pub fn close(mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap()
        }
    }
}
