#[allow(dead_code)]
#[repr(u8)]
#[derive(PartialEq, Clone, Copy)]
enum Constants {
    True = 84,         // 'T'
    False = 70,        // 'F'
    Nil = 48,          // '0'
    Fixnum = 105,      // 'i'
    Symbol = 58,       // ':'
    Symlink = 59,      // ';'
    Link = 64,         // '@'
    InstanceVar = 73,  // 'I'
    Extended = 101,    // 'e'
    Array = 91,        // '['
    Bignum = 108,      // 'l'
    Class = 99,        // 'c'
    Module = 109,      // 'm'
    ModuleOld = 77,    // 'M'
    Data = 100,        // 'd'
    Float = 102,       // 'f'
    Hash = 123,        // '{'
    HashDefault = 125, // '}'
    Object = 111,      // 'o'
    Regexp = 47,       // '/'
    String = 34,       // '"'
    Struct = 83,       // 'S'
    UserClass = 67,    // 'C'
    UserDefined = 117, // 'u'
    UserMarshal = 85,  // 'U'
    Positive = 43,     // '+'
    Negative = 45,     // '-'

    // Regular expression flags
    RegexpIgnore = 1,
    RegexpMultiline = 4,
}

impl std::ops::BitAnd<Constants> for u8 {
    type Output = u8;

    fn bitand(self, rhs: Constants) -> Self::Output {
        self & (rhs as u8)
    }
}

impl PartialEq<Constants> for u8 {
    fn eq(&self, other: &Constants) -> bool {
        *self == *other as u8
    }
}

const ENCODING_SHORT_SYMBOL: &str = "E";
const ENCODING_LONG_SYMBOL: &str = "encoding";
const EXTENDS_SYMBOL: &str = "__ruby_extends__";
const DEFAULT_SYMBOL: &str = "__ruby_default__";
const MARSHAL_VERSION: u16 = 0x0408;

pub mod dump;
pub mod load;
