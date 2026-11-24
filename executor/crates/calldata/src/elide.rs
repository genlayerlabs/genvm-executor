/// How many bytes of an untrusted string an error message keeps
pub const USER_STR_LIMIT: usize = 128;

/// Formatter output cut to `limit` bytes: past it only the head and tail halves
/// survive, joined by `...`. It never holds much more than `limit`, so formatting a
/// huge value allocates nothing of its size
pub struct Elided {
    limit: usize,
    head: String,
    head_full: bool,
    tail: String,
    total: usize,
}

impl Elided {
    pub fn format(limit: usize, args: std::fmt::Arguments<'_>) -> Result<Self, std::fmt::Error> {
        let mut ret = Self {
            limit,
            head: String::new(),
            head_full: false,
            tail: String::new(),
            total: 0,
        };
        std::fmt::Write::write_fmt(&mut ret, args)?;
        ret.trim_tail();
        Ok(ret)
    }

    pub fn head(&self) -> &str {
        &self.head
    }

    pub fn tail(&self) -> &str {
        &self.tail
    }

    pub fn is_cut(&self) -> bool {
        self.total > self.head.len() + self.tail.len()
    }

    fn tail_cap(&self) -> usize {
        self.limit - self.head.len()
    }

    fn trim_tail(&mut self) {
        if let Some(excess) = self.tail.len().checked_sub(self.tail_cap()) {
            let start = self.tail.ceil_char_boundary(excess);
            self.tail.drain(..start);
        }
    }
}

impl std::fmt::Write for Elided {
    fn write_str(&mut self, mut s: &str) -> std::fmt::Result {
        self.total += s.len();

        if !self.head_full {
            let room = self.limit / 2 - self.head.len();
            if s.len() <= room {
                self.head.push_str(s);
                return Ok(());
            }
            let take = s.floor_char_boundary(room);
            self.head.push_str(&s[..take]);
            self.head_full = true;
            s = &s[take..];
        }

        let cap = self.tail_cap();
        if s.len() >= cap {
            self.tail.clear();
            self.tail
                .push_str(&s[s.ceil_char_boundary(s.len() - cap)..]);
        } else {
            self.tail.push_str(s);
            if self.tail.len() > cap.saturating_mul(2) {
                self.trim_tail();
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for Elided {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.head)?;
        if self.is_cut() {
            f.write_str("...")?;
        }
        f.write_str(&self.tail)
    }
}

/// Formats `args` into at most `limit` bytes plus the `...` joint
pub fn format_string(limit: usize, args: std::fmt::Arguments<'_>) -> String {
    match Elided::format(limit, args) {
        Ok(e) => e.to_string(),
        Err(_) => String::from("$FormatError"),
    }
}

/// [`format_string`] at [`USER_STR_LIMIT`]
pub fn user_format(args: std::fmt::Arguments<'_>) -> String {
    format_string(USER_STR_LIMIT, args)
}

/// Widest integer [`bigint`] still shows in decimal
pub const BIGINT_DECIMAL_BITS: u64 = 256;

/// An untrusted integer as a message shows it: in decimal while that is cheap, by
/// its size past that, since converting a huge one costs superlinear time
pub fn bigint(v: &num_bigint::BigInt) -> String {
    if v.bits() <= BIGINT_DECIMAL_BITS {
        return v.to_string();
    }
    let sign = if v.sign() == num_bigint::Sign::Minus {
        "-"
    } else {
        ""
    };
    format!("{sign}<{} bit integer>", v.bits())
}

/// Interpolates an untrusted string into a message, cut to [`USER_STR_LIMIT`]
pub struct UserLogStr<'a>(pub &'a str);

impl std::fmt::Display for UserLogStr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0;
        if s.len() <= USER_STR_LIMIT {
            return f.write_str(s);
        }
        let head = s.floor_char_boundary(USER_STR_LIMIT / 2);
        let tail = s.ceil_char_boundary(s.len() - (USER_STR_LIMIT - head));
        f.write_str(&s[..head])?;
        f.write_str("...")?;
        f.write_str(&s[tail..])
    }
}
