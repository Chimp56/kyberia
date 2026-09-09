use kyberia_rendering_scene::{MAX_SCENE_BYTES, SceneDocument, SceneError};
use std::cell::RefCell;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserve a single reusable input buffer and return its WASM linear-memory
/// address. JavaScript writes exactly `len` bytes before calling
/// [`validate_scene`]. A reusable buffer keeps repeated file checks bounded by
/// the largest admitted scene instead of leaking one allocation per request.
// The two exported functions are a deliberately tiny C ABI for the browser
// worker. `unsafe(no_mangle)` is required by Rust 2024 for symbol exports; the
// implementation itself contains no unsafe operations.
#[unsafe(no_mangle)]
pub extern "C" fn input_ptr(len: u32) -> u32 {
    let len = len as usize;
    if len > MAX_SCENE_BYTES {
        return 0;
    }
    INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(len, 0);
        input.as_mut_ptr() as usize as u32
    })
}

/// Return a stable status code for the exact Rust canonical admission result.
/// The browser performs its own diagnostic validation only after this boundary
/// accepts the bytes; it never treats an unsuccessful code as a fallback.
#[unsafe(no_mangle)]
pub extern "C" fn validate_scene(len: u32) -> u32 {
    let len = len as usize;
    if len > MAX_SCENE_BYTES {
        return 5;
    }
    INPUT.with(|input| {
        let input = input.borrow();
        if input.len() != len {
            return 2;
        }
        match SceneDocument::from_canonical_bytes(&input) {
            Ok(_) => 0,
            Err(SceneError::Cancelled) => 3,
            Err(SceneError::UnsupportedVersion) => 4,
            Err(SceneError::ResourceLimit(_)) => 5,
            Err(SceneError::NonCanonicalBytes) => 6,
            Err(_) => 2,
        }
    })
}
