//! Wrapper around the game's allocator-aware note-record vector — the
//! note-*injection* half of what was once a single `game_note` module
//! (the read-only note-record layout now lives in `types::game_note`).
//!
//! The note record and its vector are owned by the game. We append to
//! the vector from within the Analyze hook, using the game's own
//! app-heap allocator — not the CRT heap, not Rust's global allocator.
//! Allocator mismatch at chart end (when the vector's destructor frees
//! its buffer) causes a heap-mismatch crash.

use std::mem;
use std::ptr;

use crate::types::game_note::GameNote;

/// Wrapper around the game's allocator-aware note-record vector that
/// supports one bulk-append.
///
/// The vector follows the standard MSVC three-pointer layout observed
/// in the Analyze disassembly:
///
/// ```text
/// +0x00  T* begin          (first element)
/// +0x08  T* end             (one past last element — size())
/// +0x10  T* end_capacity    (one past capacity — capacity())
/// ```
///
/// `append_bulk` grows the buffer via the app-heap allocator, memcpies
/// existing elements, appends the new ones, atomically updates the three
/// pointers, then frees the old buffer. This mirrors what the game's own
/// reserve + insert-at-end sequence does in the disassembly.
pub struct GameNotesVec {
    vec_ptr: *mut u8,
    heap_handle: *const u8,
    agcs_heap_malloc: unsafe extern "C" fn(*const u8, usize, usize, usize) -> *mut u8,
    agcs_heap_free: unsafe extern "C" fn(*mut u8),
}

#[derive(Debug)]
pub enum NotesVecError {
    /// Allocator returned NULL. Vector is untouched.
    AllocFailed,
    /// Vector pointer was null at construction.
    NullVector,
    /// Heap handle was null (allocator not initialized).
    NullHeap,
}

impl GameNotesVec {
    /// Construct a wrapper for the supplied note-record vector pointer.
    ///
    /// `vec_ptr` must point to a valid three-pointer MSVC-layout vector
    /// (begin/end/end_capacity at offsets 0/8/16). `heap_handle` is the
    /// dereferenced app-heap pointer (i.e. already `*app_heap_handle_addr`).
    /// Allocator function pointers come from the signature store.
    pub fn new(
        vec_ptr: *mut u8,
        heap_handle: *const u8,
        agcs_heap_malloc: unsafe extern "C" fn(*const u8, usize, usize, usize) -> *mut u8,
        agcs_heap_free: unsafe extern "C" fn(*mut u8),
    ) -> Self {
        Self {
            vec_ptr,
            heap_handle,
            agcs_heap_malloc,
            agcs_heap_free,
        }
    }

    /// Current element count (`end - begin` divided by stride).
    pub fn len(&self) -> usize {
        unsafe {
            let begin = *(self.vec_ptr as *const *mut u8);
            let end = *(self.vec_ptr.add(8) as *const *mut u8);
            if begin.is_null() || end.is_null() || end < begin {
                return 0;
            }
            (end.offset_from(begin) as usize) / mem::size_of::<GameNote>()
        }
    }

    /// Append `notes` to the vector. Always grows via the app-heap allocator
    /// (even if the existing capacity would have fit, to keep the grow path
    /// exercised for predictable behavior). One allocation, one free, one
    /// atomic pointer-triple swap.
    ///
    /// Returns `Err` on any failure; the vector is left untouched in that
    /// case (grow is transactional).
    pub fn append_bulk(&mut self, notes: &[GameNote]) -> Result<(), NotesVecError> {
        if self.vec_ptr.is_null() {
            return Err(NotesVecError::NullVector);
        }
        if self.heap_handle.is_null() {
            return Err(NotesVecError::NullHeap);
        }
        if notes.is_empty() {
            return Ok(());
        }

        let stride = mem::size_of::<GameNote>();

        unsafe {
            let begin_slot = self.vec_ptr as *mut *mut u8;
            let end_slot = self.vec_ptr.add(0x08) as *mut *mut u8;
            let cap_slot = self.vec_ptr.add(0x10) as *mut *mut u8;

            let old_begin = *begin_slot;
            let old_end = *end_slot;

            let old_size_bytes = if old_begin.is_null() {
                0
            } else {
                old_end.offset_from(old_begin) as usize
            };
            let old_count = old_size_bytes / stride;
            let add_count = notes.len();
            let new_count = old_count + add_count;
            let new_bytes = new_count * stride;

            // Allocate new buffer on the app heap. Align=0 lets the allocator
            // pick its default (matches what the compiled reserve passes).
            let new_buf = (self.agcs_heap_malloc)(self.heap_handle, new_bytes, 0, 0);
            if new_buf.is_null() {
                return Err(NotesVecError::AllocFailed);
            }

            // Copy existing elements byte-for-byte. memcpy semantics are
            // correct here because Note is #[repr(C)] with no heap-owned
            // subfields — bitwise copy is equivalent to a move.
            if old_size_bytes > 0 {
                ptr::copy_nonoverlapping(old_begin, new_buf, old_size_bytes);
            }

            // Append new entries after the existing ones.
            let append_dst = new_buf.add(old_size_bytes);
            ptr::copy_nonoverlapping(
                notes.as_ptr() as *const u8,
                append_dst,
                std::mem::size_of_val(notes),
            );

            // Swap vector pointers to the new buffer. Order: write cap, write
            // end, write begin last. This way any concurrent reader sees a
            // still-valid pre-grow layout until begin flips.
            let new_end = new_buf.add(new_count * stride);
            let new_cap = new_end; // exact-fit, no headroom
            *cap_slot = new_cap;
            *end_slot = new_end;
            *begin_slot = new_buf;

            // Release the old buffer via the paired free. The free looks up
            // the heap via the tracking header at ptr-0x18/ptr-0x20, so the
            // fact that the pointer originated from the same allocator is
            // self-describing — no heap-handle argument is needed.
            if !old_begin.is_null() {
                (self.agcs_heap_free)(old_begin);
            }
        }

        Ok(())
    }

    /// Sort the vector in-place by (beat_count, music_count).
    ///
    /// Matches the sort comparator the game's own post-parse pass runs
    /// over the Notes vector (observable in Ghidra on the Analyze
    /// function — two i32 compares at the note's +0x04 and +0x08
    /// offsets). We re-apply it here after injecting synthetic notes
    /// so the game's render/judge walkers see a consistently ordered
    /// vector:
    ///
    /// ```text
    /// if lhs.beat_count == rhs.beat_count:
    ///     return lhs.music_count < rhs.music_count
    /// else:
    ///     return lhs.beat_count < rhs.beat_count
    /// ```
    ///
    /// Sort the vector in-place by (beat_count, music_count).
    ///
    /// Matches the sort comparator the game's own post-parse pass runs
    /// over the Notes vector (observable in Ghidra on the Analyze
    /// function — two i32 compares at the note's +0x04 and +0x08
    /// offsets). We re-apply it here after injecting synthetic notes
    /// so the game's render/judge walkers see a consistently ordered
    /// vector:
    ///
    /// ```text
    /// if lhs.beat_count == rhs.beat_count:
    ///     return lhs.music_count < rhs.music_count
    /// else:
    ///     return lhs.beat_count < rhs.beat_count
    /// ```
    ///
    /// We call this after the NoteTypeRegistry has dispatched
    /// on_chart_loaded to all registered types. At that point the Notes
    /// vector has the vanilla-sorted regular notes plus our appended
    /// mine entries — sorting restores the (beat_count, music_count)
    /// invariant the game relies on for its render collector's early-
    /// out on off-screen entries.
    ///
    /// Safe to call with an empty or single-element vector (no-op). Reads
    /// the current begin/end pointers, slices the buffer, and sorts via
    /// Rust's pdqsort. No allocation, no game-function calls.
    pub fn sort_by_beat_and_music_count(&mut self) {
        if self.vec_ptr.is_null() {
            return;
        }
        unsafe {
            let begin = *(self.vec_ptr as *const *mut u8);
            let end = *(self.vec_ptr.add(0x08) as *const *mut u8);
            if begin.is_null() || end <= begin {
                return;
            }
            let count = (end.offset_from(begin) as usize) / mem::size_of::<GameNote>();
            if count < 2 {
                return;
            }
            let slice = std::slice::from_raw_parts_mut(begin as *mut GameNote, count);
            slice.sort_by(|a, b| {
                a.beat_count
                    .cmp(&b.beat_count)
                    .then_with(|| a.music_count.cmp(&b.music_count))
            });
        }
    }
}
