use concat_idents::concat_idents;

pub trait IntoIntComptime<T> {
    fn into_int_comptime(self) -> T;
}

pub trait IntoIntDowncast<T>
where
    T: Sized + Copy,
    Self: Sized + Copy,
{
    fn into_int_downcast(self) -> std::result::Result<T, std::num::TryFromIntError>;
    fn into_int_downcast_panicking(self) -> T {
        self.into_int_downcast().unwrap()
    }
}

impl<F, T> IntoIntDowncast<T> for F
where
    T: Sized + TryFrom<F> + Copy,
    T::Error: Into<std::num::TryFromIntError>,
    F: Sized + Copy,
{
    fn into_int_downcast(self) -> std::result::Result<T, std::num::TryFromIntError> {
        T::try_from(self).map_err(Into::into)
    }
}

const fn typecheck<T: Copy>(_: impl IntoIntDowncast<T>) {}

const _: () = typecheck::<u32>(0_usize);

macro_rules! declare_caster {
    ($t1:ident, $t2:ident) => {
        const _: () = {
            assert!($t1::BITS <= $t2::BITS, concat!(stringify!($t1), " cannot fit into ", stringify!($t2)));
        };

        concat_idents!(name = $t1, _into_, $t2 {
            pub const fn name(x: $t1) -> $t2 {
                x as $t2
            }
        });

        impl IntoIntComptime<$t2> for $t1 {
            fn into_int_comptime(self) -> $t2 {
                self as $t2
            }
        }
    };
}

// only pairs std has no `From` for, i.e. the platform-dependent ones;
// together they pin usize to exactly 64 bits
declare_caster!(u32, usize);
declare_caster!(u64, usize);
declare_caster!(usize, u64);
declare_caster!(usize, u128);
