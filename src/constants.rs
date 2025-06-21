#[allow(dead_code)]
#[repr(u8)]
#[derive(PartialEq, Clone, Copy)]
pub enum Constants {
    True = b'T',
    False = b'F',
    Null = b'0',
    SmallInt = b'i',
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

// Type prefixes
pub const NULL_PREFIX: &str = "__null__";
pub const BOOLEAN_PREFIX: &str = "__boolean__";
pub const INTEGER_PREFIX: &str = "__integer__";
pub const FLOAT_PREFIX: &str = "__float__";
pub const ARRAY_PREFIX: &str = "__array__";
pub const OBJECT_PREFIX: &str = "__object__";
pub const SYMBOL_PREFIX: &str = "__symbol__";
pub const PREFIXES: [&str; 7] = [
    NULL_PREFIX,
    BOOLEAN_PREFIX,
    INTEGER_PREFIX,
    FLOAT_PREFIX,
    ARRAY_PREFIX,
    OBJECT_PREFIX,
    SYMBOL_PREFIX,
];

// Required constants
pub const UTF8_ENCODING_SYMBOL: &str = "__symbol__E";
pub const NON_UTF8_ENCODING_SYMBOL: &str = "__symbol__encoding";
pub const EXTENDS_SYMBOL: &str = "__ruby_extends__";
pub const DEFAULT_SYMBOL: &str = "__ruby_default__";
pub const MARSHAL_VERSION: &[u8; 2] = &0x0408u16.to_be_bytes(); // The latest and probably final version of Ruby Marshal is 4.8

pub(crate) const NUMBER_PADDING: i32 = 5;
