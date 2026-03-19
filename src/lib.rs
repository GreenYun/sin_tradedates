//! Usage: See `examples/main.rs`

use std::{backtrace::Backtrace, error, fmt, ops};

use chrono::NaiveDate;

/// A parser accepts the input encoded string with simple conversion and validation.
#[derive(Debug)]
pub struct Parser {
    raw: Vec<u8>,
}

impl Parser {
    /// Create a new parser from the input string.
    ///
    /// # Errors
    ///
    /// Returns [`ParserError::InputCharacter`] if the input string contains invalid characters.
    pub fn new(s: &str) -> Result<Self, ParserError> {
        let raw = s
            .chars()
            .map(Self::decode_base64_char)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { raw })
    }

    /// Put the parser into [parsing state](ParseState), which can be used to collect all trade
    /// dates or iterate through them.
    ///
    /// # Errors
    ///
    /// Returns [`ParserError::Magic`] if the magic number is invalid.
    ///
    /// Returns [`ParserError::Size`] if the size of the data is invalid.
    ///
    /// Returns [`ParserError::DataCorruption`] if the data is corrupted.
    pub fn parse(&'_ self) -> Result<ParseState<'_>, ParserError> {
        ParseState::new(self)
    }

    fn decode_base64_char(c: char) -> Result<u8, ParserError> {
        match c {
            'A'..='Z' => Ok((c as u8) - b'A'),
            'a'..='z' => Ok(26 + (c as u8) - b'a'),
            '0'..='9' => Ok(52 + (c as u8) - b'0'),
            '+' => Ok(62),
            '/' => Ok(63),
            _ => Err(ParserError::input_character(c)),
        }
    }
}

/// The parsing state, which contains the necessary information to collect or iterate through trade
/// dates.
#[derive(Debug)]
pub struct ParseState<'a> {
    first_day: i32,
    last_day: i32,

    bit_reader: BitReader<'a>,
}

impl<'a> ParseState<'a> {
    fn new(parser: &'a Parser) -> Result<Self, ParserError> {
        let mut bit_reader = BitReader::new(&parser.raw);
        let magic = (
            bit_reader
                .read_i32(12)
                .ok_or_else(ParserError::data_corruption)?,
            bit_reader
                .read_i32(6)
                .ok_or_else(ParserError::data_corruption)?,
        );

        if magic.0 != 139 || magic.1 != 63 {
            return Err(ParserError::magic(magic.0, magic.1));
        }

        let first_day = bit_reader
            .read_i32(18)
            .ok_or_else(ParserError::data_corruption)?;
        let last_day = bit_reader
            .read_i32(18)
            .ok_or_else(ParserError::data_corruption)?;

        if first_day > last_day {
            return Err(ParserError::size(first_day, last_day));
        }

        Ok(Self {
            first_day,
            last_day,

            bit_reader,
        })
    }

    /// Try to convert the parsing state into an iterator of trade dates.
    ///
    /// # Errors
    ///
    /// Returns [`ParserError::DataCorruption`] if the data is corrupted.
    pub fn try_into_iter(mut self) -> Result<ParseIter<'a>, ParserError> {
        let current_day = self.first_day;

        let Ok((last_read_size, remaining_consecutive)) = self.get_days_series(0) else {
            return Err(ParserError::data_corruption());
        };

        Ok(ParseIter {
            state: self,

            current_day,
            last_read_size,
            remaining_consecutive,
        })
    }

    /// Collect all trade dates into a vector.
    ///
    /// # Errors
    ///
    /// Returns [`ParserError::DataCorruption`] if the data is corrupted.
    #[allow(clippy::cast_sign_loss)]
    pub fn collect(mut self) -> Result<Vec<NaiveDate>, ParserError> {
        let estimated = ((self.last_day - self.first_day) * 2 / 3) as usize;
        let mut dates = Vec::with_capacity(estimated);

        let (mut read_size, mut series) = self.get_days_series(0)?;

        for day in self.first_day..=self.last_day {
            let w = day % 7;
            if matches!(w, 3 | 4) {
                // Skip Saturday and Sunday
                continue;
            }

            if series <= 0 {
                (read_size, series) = self.get_days_series(read_size)?;
            } else {
                let date = Self::to_naive_date(day);
                dates.push(date);
                series -= 1;
            }
        }

        dates.shrink_to_fit();

        Ok(dates)
    }

    #[allow(clippy::cast_sign_loss)]
    fn get_days_series(&mut self, read_size: i32) -> Result<(i32, i32), ParserError> {
        let mut y = || {
            self.bit_reader
                .next_bit()
                .ok_or_else(ParserError::data_corruption)
        };

        let has_diff = y()?;
        let diff = if has_diff {
            let sign = if y()? { 1 } else { -1 };

            let mut bits = 1;
            while y()? {
                bits += 1;
            }

            sign * bits
        } else {
            0
        };

        let read_size = read_size + diff;

        if read_size < 0 {
            return Err(ParserError::data_corruption());
        }

        let bits = 3usize
            .checked_mul(read_size as usize)
            .ok_or_else(ParserError::data_corruption)?;

        let days = self
            .bit_reader
            .read_i32(bits)
            .ok_or_else(ParserError::data_corruption)?;

        Ok((read_size, days))
    }

    const fn to_naive_date(day: i32) -> NaiveDate {
        const SKIP_DAYS: i32 = 7657;
        NaiveDate::from_epoch_days(SKIP_DAYS + day).expect("should never fail")
    }
}

/// Trade date iterator.
pub struct ParseIter<'a> {
    state: ParseState<'a>,

    current_day: i32,
    last_read_size: i32,
    remaining_consecutive: i32,
}

impl Iterator for ParseIter<'_> {
    type Item = Result<NaiveDate, ParserError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_day > self.state.last_day {
            return None;
        }

        loop {
            let w = self.current_day % 7;
            if matches!(w, 3 | 4) {
                self.current_day += 5 - w;
            }

            if self.remaining_consecutive <= 0 {
                (self.last_read_size, self.remaining_consecutive) =
                    match self.state.get_days_series(self.last_read_size) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };

                self.current_day += 1;
            } else {
                break;
            }
        }

        let date = ParseState::to_naive_date(self.current_day);

        self.remaining_consecutive -= 1;
        self.current_day += 1;

        Some(Ok(date))
    }
}

#[derive(Debug)]
struct BitReader<'a> {
    raw: &'a [u8],
    by: usize,
    bi: usize,
}

impl<'a> BitReader<'a> {
    const BITS_PER_BYTE: usize = 6;

    const fn new(raw: &'a [u8]) -> Self {
        Self { raw, by: 0, bi: 0 }
    }

    fn next_bit(&mut self) -> Option<bool> {
        if self.by >= self.raw.len() {
            return None;
        }

        let bit = (self.raw[self.by] >> self.bi) & 1 != 0;
        *self += 1;

        Some(bit)
    }

    fn read_i32(&mut self, bits: usize) -> Option<i32> {
        if bits > i32::BITS as usize {
            return None;
        }

        if bits == 0 {
            return Some(0);
        }

        if self.by >= self.raw.len() {
            return None;
        }

        let mut num = 0;
        let mut remaining = bits;
        let mut offset = 0;

        while remaining > 0 {
            if self.by >= self.raw.len() {
                return None;
            }

            let available = Self::BITS_PER_BYTE - self.bi;
            let take = remaining.min(available);
            let mask = (1u8 << take) - 1;
            let chunk: i32 = ((self.raw[self.by] >> self.bi) & mask).into();

            num |= chunk << offset;
            *self += take;

            remaining -= take;
            offset += take;
        }

        Some(num)
    }
}

impl ops::Add<usize> for BitReader<'_> {
    type Output = Self;

    fn add(mut self, rhs: usize) -> Self::Output {
        let bits = self.bi + rhs;
        self.by += bits / Self::BITS_PER_BYTE;
        self.bi = bits % Self::BITS_PER_BYTE;
        self
    }
}

impl ops::AddAssign<usize> for BitReader<'_> {
    fn add_assign(&mut self, rhs: usize) {
        let bits = self.bi + rhs;
        self.by += bits / Self::BITS_PER_BYTE;
        self.bi = bits % Self::BITS_PER_BYTE;
    }
}

/// Errors that may occur during parsing.
#[derive(Debug)]
pub enum ParserError {
    InputCharacter(char, Backtrace),
    Magic(i32, i32, Backtrace),
    Size(i32, i32, Backtrace),
    DataCorruption(Backtrace),
}

impl ParserError {
    fn input_character(c: char) -> Self {
        Self::InputCharacter(c, Backtrace::capture())
    }

    fn magic(m1: i32, m2: i32) -> Self {
        Self::Magic(m1, m2, Backtrace::capture())
    }

    fn size(s1: i32, s2: i32) -> Self {
        Self::Size(s1, s2, Backtrace::capture())
    }

    fn data_corruption() -> Self {
        Self::DataCorruption(Backtrace::capture())
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputCharacter(c, backtrace) => {
                write!(f, "Invalid input character: {c}, {backtrace}")
            }

            Self::Magic(m1, m2, backtrace) => {
                write!(f, "Invalid magic: {m1}, {m2}, {backtrace}")
            }

            Self::Size(s1, s2, backtrace) => {
                write!(f, "Invalid size: {s1}, {s2}, {backtrace}")
            }

            Self::DataCorruption(backtrace) => {
                write!(f, "Data corruption: {backtrace}")
            }
        }
    }
}

impl error::Error for ParserError {}
