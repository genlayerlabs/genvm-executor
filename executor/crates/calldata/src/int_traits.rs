// a copy of the executor-side module, kept dependency-free; keep the two in sync

pub trait IntoIntComptime<T> {
    fn into_int_comptime(self) -> T;
}

macro_rules! declare_caster {
    ($t1:ident, $t2:ident) => {
        const _: () = {
            assert!(
                $t1::BITS <= $t2::BITS,
                concat!(stringify!($t1), " cannot fit into ", stringify!($t2))
            );
        };

        impl IntoIntComptime<$t2> for $t1 {
            fn into_int_comptime(self) -> $t2 {
                self as $t2
            }
        }
    };
}

// only pairs std has no `From` for; all three hold on 32-bit targets too, so
// this stays usable from wasm32 guests
declare_caster!(u32, usize);
declare_caster!(usize, u64);
declare_caster!(usize, u128);
