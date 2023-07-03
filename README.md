# Stupid alloc - what if memory allocation was annoying
Mostly a weird exercise in how much you can make a memory allocator suck balls.
Included in this repository is the memory allocator code, along with an empty main.
This is more of a proof of concept than a fully standalone library (and i don't know why anyone would want this as a dependency), but I guess it's fun to learn about memory allocation and see what really is inside the memory.

This memory allocator will prompt you for a file to store the allocated memory in. The memory will indeed be stored in the file (as a memory-map), and be deleted when free'd (you also get a little message when that happens).

## Disclaimers
- I only tested this on Windows; if it doesn't work on MacOS or Linux, message me and i'll **maybe** look into it (probably not for MacOS I don't have one)
- If you mess with the memory files in any way you'll mess up with your program memory, but seeing as this is topologically the same as messing with `/proc/mem` I consider this a cool feature.
- I'm probably going to work on this *a little bit more* to add some quality-of-life features, but that's it. It's a shitpost, not a serious library.

## (old) Demo
https://github.com/shadyfennec/stupidalloc/assets/68575248/f2490dc1-8412-4450-9359-7387f79682ea
