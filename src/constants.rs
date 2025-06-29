#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum Constants {
    True = b'T',
    False = b'F',
    Null = b'0',
    Int = b'i',
    Symbol = b':',
    SymbolLink = b';',
    ObjectLink = b'@',
    InstanceVar = b'I',
    ExtendedObject = b'e',
    Array = b'[',
    BigInt = b'l',
    Class = b'c',
    Module = b'm',
    ModuleOld = b'M',
    Data = b'd',
    Float = b'f',
    HashMap = b'{',
    HashMapDefault = b'}',
    Object = b'o',
    Regexp = b'/',
    String = b'"',
    Struct = b'S',
    UserClass = b'C',
    UserDefined = b'u',
    UserMarshal = b'U',
    SignPositive = b'+',
    SignNegative = b'-',

    RegexpIgnore = 1,
    RegexpExtended = 2,
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

impl From<Constants> for u8 {
    fn from(val: Constants) -> Self {
        val as u8
    }
}

// Required constants
pub const UTF8_ENCODING_SYMBOL: &str = "E";
pub const NON_UTF8_ENCODING_SYMBOL: &str = "encoding";
pub const DEFAULT_SYMBOL: &str = "__ruby_default__";
pub const MARSHAL_VERSION: &[u8; 2] = &0x0408u16.to_be_bytes(); // The latest and probably final version of Ruby Marshal is 4.8

pub const NUMBER_PADDING: u8 = 5;
