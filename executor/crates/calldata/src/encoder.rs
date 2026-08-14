use crate::Address;
use crate::consts::*;
use crate::int_traits::IntoIntComptime;

fn write_uleb<W: Writer>(w: &mut W, mut num: num_bigint::BigUint) -> Result<(), W::Error> {
    if num == num_bigint::BigUint::ZERO {
        return w.write_one(0);
    }

    loop {
        let mut cur = (num.iter_u32_digits().next().unwrap_or(0) & 0x7f) as u8;
        num >>= 7u32;
        let has_next = num != num_bigint::BigUint::ZERO;

        if has_next {
            cur |= 0x80;
        }

        w.write_one(cur)?;

        if !has_next {
            return Ok(());
        }
    }
}

fn write_uleb_u64<W: Writer>(w: &mut W, mut num: u64) -> Result<(), W::Error> {
    if num == 0 {
        return w.write_one(0);
    }

    loop {
        let mut cur = (num & 0x7f) as u8;
        num >>= 7;

        if num != 0 {
            cur |= 0x80;
        }

        w.write_one(cur)?;

        if num == 0 {
            return Ok(());
        }
    }
}

fn write_uleb_u128<W: Writer>(w: &mut W, mut num: u128) -> Result<(), W::Error> {
    if num == 0 {
        return w.write_one(0);
    }

    loop {
        let mut cur = (num & 0x7f) as u8;
        num >>= 7;

        if num != 0 {
            cur |= 0x80;
        }

        w.write_one(cur)?;

        if num == 0 {
            return Ok(());
        }
    }
}

fn write_uleb_tagged_u64<W: Writer>(w: &mut W, tag: u8, val: u64) -> Result<(), W::Error> {
    let val = u128::from(val);
    let bits_in_type: u128 = BITS_IN_TYPE.into_int_comptime();
    write_uleb_u128(w, (val << bits_in_type) + u128::from(tag))
}

fn write_tagged_uleb<W: Writer>(
    w: &mut W,
    tag: u8,
    val: num_bigint::BigUint,
) -> Result<(), W::Error> {
    write_uleb(w, (val << BITS_IN_TYPE) + tag)
}

pub trait Writer {
    type Error;

    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error>;

    fn write_one(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.write_all(&[byte])
    }
}

pub struct CounterWriter(pub u64);

impl Writer for CounterWriter {
    type Error = std::convert::Infallible;

    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let len: u64 = data.len().into_int_comptime();
        self.0 += len;
        Ok(())
    }
}

pub struct StdWriter<W>(W)
where
    W: std::io::Write;

impl<W: std::io::Write> Writer for StdWriter<W> {
    type Error = std::io::Error;

    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.0.write_all(data)
    }
}

impl<W> From<W> for StdWriter<W>
where
    W: std::io::Write,
{
    fn from(writer: W) -> Self {
        Self(writer)
    }
}

impl<W> StdWriter<W>
where
    W: std::io::Write,
{
    pub fn into_inner(self) -> W {
        self.0
    }

    pub fn new(writer: W) -> Self {
        Self(writer)
    }
}

/// Debug-only bookkeeping for a container being written, used to assert that the
/// caller produces canonical calldata (see [`Encoder`]). Compiled out entirely in
/// release builds.
#[cfg(debug_assertions)]
enum EncoderDebugFrame {
    Array {
        remaining: u64,
    },
    Map {
        remaining: u64,
        prev_key: Option<String>,
        awaiting_value: bool,
    },
}

pub struct Encoder<W>
where
    W: Writer,
{
    writer: W,
    /// Stack of currently-open containers. Each scalar/value push consumes one slot
    /// of the innermost container (cascading up as containers fill), each `push_map_k`
    /// checks strict key ordering. Debug-only: the whole guard compiles out in release.
    #[cfg(debug_assertions)]
    debug_frames: Vec<EncoderDebugFrame>,
}

impl<W: Writer> Encoder<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            #[cfg(debug_assertions)]
            debug_frames: Vec::new(),
        }
    }

    pub fn into_inner(self) -> W {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.debug_frames.is_empty(),
            "calldata encoder: unfinished container(s) on into_inner (fewer entries pushed than declared)"
        );
        self.writer
    }

    /// A complete value was just written into the innermost container. Consume one
    /// slot, cascading up as containers reach their declared size.
    #[cfg(debug_assertions)]
    fn dbg_value(&mut self) {
        loop {
            match self.debug_frames.last_mut() {
                None => return,
                Some(EncoderDebugFrame::Array { remaining }) => {
                    debug_assert!(*remaining > 0, "calldata encoder: too many array elements");
                    *remaining -= 1;
                    if *remaining > 0 {
                        return;
                    }
                }
                Some(EncoderDebugFrame::Map {
                    remaining,
                    awaiting_value,
                    ..
                }) => {
                    debug_assert!(
                        *awaiting_value,
                        "calldata encoder: value pushed but a map key was expected"
                    );
                    *awaiting_value = false;
                    *remaining -= 1;
                    if *remaining > 0 {
                        return;
                    }
                }
            }
            self.debug_frames.pop();
        }
    }

    /// A container header for `len` elements was just written.
    #[cfg(debug_assertions)]
    fn dbg_begin_container(&mut self, is_map: bool, len: u64) {
        if len == 0 {
            // An empty container is itself a complete value in its parent.
            self.dbg_value();
        } else if is_map {
            self.debug_frames.push(EncoderDebugFrame::Map {
                remaining: len,
                prev_key: None,
                awaiting_value: false,
            });
        } else {
            self.debug_frames
                .push(EncoderDebugFrame::Array { remaining: len });
        }
    }

    /// A map key was just written.
    #[cfg(debug_assertions)]
    fn dbg_key(&mut self, key: &str) {
        match self.debug_frames.last_mut() {
            Some(EncoderDebugFrame::Map {
                remaining,
                prev_key,
                awaiting_value,
            }) => {
                debug_assert!(
                    !*awaiting_value,
                    "calldata encoder: map key pushed but a value was expected"
                );
                debug_assert!(*remaining > 0, "calldata encoder: too many map entries");
                if let Some(prev) = prev_key.as_deref() {
                    debug_assert!(
                        key > prev,
                        "calldata encoder: map keys must be strictly increasing, got {prev:?} then {key:?}"
                    );
                }
                *prev_key = Some(key.to_owned());
                *awaiting_value = true;
            }
            _ => debug_assert!(false, "calldata encoder: push_map_k outside of a map"),
        }
    }

    pub fn start_array(&mut self, len: u64) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_begin_container(false, len);
        write_uleb_tagged_u64(&mut self.writer, TYPE_ARR, len)
    }

    pub fn start_array_big(&mut self, len: &num_bigint::BigInt) -> Result<(), W::Error> {
        let len = len.to_biguint().expect("array length must be non-negative");
        #[cfg(debug_assertions)]
        self.dbg_begin_container(false, u64::try_from(&len).unwrap_or(u64::MAX));
        write_tagged_uleb(&mut self.writer, TYPE_ARR, len)
    }

    pub fn start_map(&mut self, len: u64) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_begin_container(true, len);
        write_uleb_tagged_u64(&mut self.writer, TYPE_MAP, len)
    }

    pub fn start_map_big(&mut self, len: &num_bigint::BigInt) -> Result<(), W::Error> {
        let len = len.to_biguint().expect("map length must be non-negative");
        #[cfg(debug_assertions)]
        self.dbg_begin_container(true, u64::try_from(&len).unwrap_or(u64::MAX));
        write_tagged_uleb(&mut self.writer, TYPE_MAP, len)
    }

    pub fn push_null(&mut self) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        self.writer.write_all(&[SPECIAL_NULL])
    }

    pub fn push_bool(&mut self, value: bool) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        self.writer
            .write_all(&[if value { SPECIAL_TRUE } else { SPECIAL_FALSE }])
    }

    pub fn push_i64(&mut self, value: i64) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        if value < 0 {
            write_uleb_tagged_u64(&mut self.writer, TYPE_NINT, -(value + 1) as u64)
        } else {
            write_uleb_tagged_u64(&mut self.writer, TYPE_PINT, value as u64)
        }
    }

    pub fn push_u64(&mut self, value: u64) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        write_uleb_tagged_u64(&mut self.writer, TYPE_PINT, value)
    }

    pub fn push_bigint(&mut self, value: &num_bigint::BigInt) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        if value.sign() == num_bigint::Sign::Minus {
            let mut mag = value.magnitude().clone();
            mag -= 1u32;
            write_tagged_uleb(&mut self.writer, TYPE_NINT, mag)
        } else {
            write_tagged_uleb(&mut self.writer, TYPE_PINT, value.magnitude().clone())
        }
    }

    pub fn push_str(&mut self, value: &str) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        write_uleb_tagged_u64(&mut self.writer, TYPE_STR, value.len().into_int_comptime())?;
        self.writer.write_all(value.as_bytes())
    }

    pub fn push_bytes(&mut self, value: &[u8]) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        write_uleb_tagged_u64(
            &mut self.writer,
            TYPE_BYTES,
            value.len().into_int_comptime(),
        )?;
        self.writer.write_all(value)
    }

    pub fn write_raw(&mut self, data: &[u8]) -> Result<(), W::Error> {
        // `write_raw` splices an already-encoded, complete value (see the `unparsed`
        // codec), so from the surrounding container's view it is one value.
        #[cfg(debug_assertions)]
        self.dbg_value();
        self.writer.write_all(data)
    }

    pub fn push_address(&mut self, addr: &Address) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_value();
        self.writer.write_all(&[SPECIAL_ADDR])?;
        self.writer.write_all(&addr.0)
    }

    pub fn push_map_k(&mut self, key: &str) -> Result<(), W::Error> {
        #[cfg(debug_assertions)]
        self.dbg_key(key);
        write_uleb_u64(&mut self.writer, key.len().into_int_comptime())?;
        self.writer.write_all(key.as_bytes())
    }
}

// The map-canonicity guard is a `debug_assert`, so these panics only exist in debug
// builds; gate the tests accordingly.
#[cfg(all(test, debug_assertions))]
mod debug_guard_tests {
    use super::*;

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn out_of_order_map_keys_trip_the_guard() {
        let mut enc = Encoder::new(Vec::new());
        enc.start_map(2).unwrap();
        enc.push_map_k("b").unwrap();
        enc.push_null().unwrap();
        // "a" < "b": not strictly increasing.
        enc.push_map_k("a").unwrap();
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn duplicate_map_keys_trip_the_guard() {
        let mut enc = Encoder::new(Vec::new());
        enc.start_map(2).unwrap();
        enc.push_map_k("a").unwrap();
        enc.push_null().unwrap();
        enc.push_map_k("a").unwrap();
    }

    #[test]
    #[should_panic(expected = "fewer entries")]
    fn under_filled_map_trips_on_into_inner() {
        let mut enc = Encoder::new(Vec::new());
        enc.start_map(2).unwrap();
        enc.push_map_k("a").unwrap();
        enc.push_null().unwrap();
        // Declared 2 pairs but only pushed 1.
        let _ = enc.into_inner();
    }

    #[test]
    fn well_formed_nested_containers_pass() {
        let mut enc = Encoder::new(Vec::new());
        enc.start_map(2).unwrap();
        enc.push_map_k("a").unwrap();
        enc.start_array(2).unwrap();
        enc.push_u64(1).unwrap();
        enc.start_map(0).unwrap(); // empty map as an array element
        enc.push_map_k("b").unwrap();
        enc.push_null().unwrap();
        let _ = enc.into_inner();
    }
}
