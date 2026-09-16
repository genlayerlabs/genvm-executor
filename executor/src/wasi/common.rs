pub fn align_slice(slice: &mut [u8], alignment: usize) -> &mut [u8] {
    let ptr = slice.as_ptr() as usize;
    let aligned = (ptr + alignment - 1) & !(alignment - 1);
    let offset = aligned - ptr;

    if offset >= slice.len() {
        return &mut [];
    }

    &mut slice[offset..]
}
