# coding: utf-8
#
# Writes the Ruby 4 fixtures `tests/ruby_fixtures.rs` round-trips, and a `manifest.json` saying what is in each.
# Usage: `ruby tests/fixtures/generate.rb <output dir>`.
#
# Everything is the real `Marshal.dump` output of this Ruby, except the `float_synthetic_*` group, which Ruby 4
# cannot write: floats followed by a NUL and "mantissa" bytes, the way Ruby 1.8 and 1.9.2 wrote them (see
# generate_ruby18.rb for the real thing from Ruby 1.8.7). Those are assembled byte by byte, in the layouts seen
# in real RPG Maker VX Ace data, plus tails chosen to be invalid UTF-8 in every way that matters.

require "fileutils"
require "set"
require "zlib"
require_relative "ruby_common"
require_relative "classes"

# Ruby before 2.0 has no String#b; the bytes are binary either way.
unless "".respond_to?(:b)
  class String
    def b; dup.force_encoding("ASCII-8BIT"); end
  end
end

OUT = ARGV.fetch(0) { abort "usage: ruby generate.rb <output dir>" }
FileUtils.mkdir_p(OUT)

FIXTURES = []

def record(name, bytes, group, desc, extra = {})
  raise "duplicate fixture #{name}" if FIXTURES.any? { |fixture| fixture["name"] == name }
  File.binwrite(File.join(OUT, "#{name}.bin"), bytes.b)
  FIXTURES << { "name" => name, "group" => group, "desc" => desc }.merge(extra)
end

# A Ruby value, dumped by Ruby and described by it.
def emit(name, group, object, extra = {})
  record(name, Marshal.dump(object), group, Describe.desc(object), extra)
end

# Bytes Ruby did not write itself (nothing is described).
def emit_raw(name, group, bytes, extra = {})
  record(name, bytes, group, nil, extra)
end

# ---------------------------------------------------------------------------------------------------------
# A tiny Marshal 4.8 writer, only for the floats Ruby 4 can no longer write.
# ---------------------------------------------------------------------------------------------------------
class Wire
  def initialize; (@out = "\x04\x08".b); end
  def bytes; @out.dup; end

  # marshal.c `w_long`.
  def self.long(n)
    return "\0".b if n == 0
    return [n + 5].pack("C") if n > 0 && n < 123
    return [(n - 5) & 255].pack("C") if n < 0 && n > -124

    buf = []
    4.times do
      buf << (n & 255)
      n >>= 8
      return ([buf.size].pack("C") + buf.pack("C*")) if n == 0
      return ([-buf.size & 255].pack("C") + buf.pack("C*")) if n == -1
    end
    raise "not a 32-bit integer"
  end

  def raw(data); (@out << data.b; self); end
  def long(n); raw(Wire.long(n)); end
  def tag(char); raw(char); end
  def chunk(data); (long(data.bytesize); raw(data)); end

  # `tail` is everything after the text, NUL included (what `save_mantissa` returns).
  def float(text, tail = ""); (tag("f"); chunk(text.b + tail.b)); end
  def array(count); (tag("["); long(count); yield); end
  def link(index); (tag("@"); long(index)); end
end

# The generator trusts nothing it cannot check: `Wire.long` must agree with Ruby's own encoder.
[0, 1, -1, 122, 123, -123, -124, 255, 256, -256, 65_535, 65_536, 2**24, 2**30 - 1, -(2**30), -(2**24) - 1].each do |n|
  raise "Wire.long(#{n}) is wrong" unless Wire.long(n) == Marshal.dump(n)[3..-1].b
end

# ---------------------------------------------------------------------------------------------------------
# Plain values: what every Marshal reader has to get right.
# ---------------------------------------------------------------------------------------------------------
emit("basic_nil", "basic", nil)
emit("basic_true", "basic", true)
emit("basic_false", "basic", false)

FIXNUMS = [0, 1, -1, 5, -5, 122, 123, -123, -124, 255, -255, 256, -256, 65_535, 65_536, -65_536, 2**24, -(2**24),
           2**30 - 1, -(2**30)]
FIXNUMS.each { |n| emit("basic_int_#{n.to_s.tr('-', 'm')}", "basic", n) }
emit("basic_fixnum_array", "basic", FIXNUMS)

BIGNUMS = [2**30, -(2**30) - 1, 2**31, -(2**31) - 1, 2**32, 2**62, 2**64, -(2**64), 2**100, -(2**100), 2**1000, 2**65 - 1]
BIGNUMS.each_with_index { |n, i| emit("basic_bignum_#{i}", "basic", n) }
emit("basic_bignum_array", "basic", BIGNUMS)

emit("basic_array_mixed", "basic", [1, "two", 3.0, [4], { 5 => 6 }, :seven, nil, true, false])
emit("basic_hash", "basic", { 1 => "one", :two => 2, "three" => [3], nil => nil, 4.5 => { 6 => 7 } })
emit("basic_hash_default", "basic", Hash.new("default").merge(:a => 1))
emit("basic_empty_containers", "basic", [[], {}, "", :a])

shared_string = "shared"
shared_array = [1, 2]
emit("basic_links", "basic", [shared_string, shared_string, shared_array, shared_array, [shared_string]])
emit("basic_symbol_links", "basic", [:a, :b, :a, { :a => :b, :b => :a }, [:a, :b]])
emit("basic_symbol_link_keys", "basic", [{ :alpha => 1, :beta => 2 }, { :alpha => 3, :beta => 4 }, { :alpha => 5, :beta => 6 }])

cycle = [1, shared_string, shared_string, :a, :a]
cycle << cycle
emit("basic_cycle", "basic", cycle)
hash_cycle = { :name => "loop" }
hash_cycle[:self] = hash_cycle
emit("basic_hash_cycle", "basic", hash_cycle)

# 250 levels is under every depth limit; the serde envelope allows 256.
nested = nil
250.times { nested = [nested] }
emit("basic_deeply_nested", "basic", nested)
nested = nil
12.times { nested = [nested] }
emit("basic_nested_12", "basic", nested)

# Several streams in one file, as `load_many` reads them.
emit_raw("many_streams", "many", Marshal.dump(1) + Marshal.dump("two") + Marshal.dump([3, :four]) + Marshal.dump(nil))

# ---------------------------------------------------------------------------------------------------------
# Strings in many encodings.
# ---------------------------------------------------------------------------------------------------------
STRINGS = {
  "ascii" => "plain ascii text",
  "empty" => "",
  "utf8" => "héllo wörld 日本語のテキスト 🎉",
  "utf8_nul" => "a\0b\0\0c",
  "utf8_invalid" => "\xff\xfe broken \xc3".force_encoding("UTF-8"),
  # RPG Maker's own compressed script data: tagged UTF-8 while being anything but text.
  "utf8_zlib" => Zlib::Deflate.deflate("Scripts " * 40).force_encoding("UTF-8"),
  "shift_jis" => "日本語のテキストです".encode("Shift_JIS"),
  "shift_jis_empty" => "".encode("Shift_JIS"),
  "windows_31j" => "①②③ ㈱ 髙".encode("Windows-31J"),
  "euc_jp" => "日本語のテキスト".encode("EUC-JP"),
  "gbk" => "汉字内容".encode("GBK"),
  "gb18030" => "汉字🎉".encode("GB18030"),
  "big5" => "漢字內容".encode("Big5"),
  "euc_kr" => "한국어 텍스트".encode("EUC-KR"),
  "iso_8859_1" => "café ñ ü".encode("ISO-8859-1"),
  "iso_8859_2" => "żółć".encode("ISO-8859-2"),
  "windows_1252" => "€uro “quoted” – dash".encode("Windows-1252"),
  "windows_1251" => "Привет".encode("Windows-1251"),
  "koi8_r" => "Привет".encode("KOI8-R"),
  "utf16le" => "aé日🎉".encode("UTF-16LE"),
  "utf16be" => "aé日🎉".encode("UTF-16BE"),
  "utf32le" => "aé日🎉".encode("UTF-32LE"),
  "utf32be" => "aé日🎉".encode("UTF-32BE"),
  "us_ascii" => "ascii only".force_encoding("US-ASCII"),
  "binary" => "\x00\x01\xfe\xff binary".b,
  "binary_sjis_bytes" => "日本語".encode("Shift_JIS").b,
  "shift_jis_invalid" => "\x83".force_encoding("Shift_JIS"),
  "long_shift_jis" => "あいうえお".encode("Shift_JIS") * 20_000
}

STRINGS.each { |name, value| emit("str_#{name}", "string", value) }

# Several strings of the same legacy encoding in one document: Ruby shares the encoding's name between them.
emit("str_repeated_encodings", "string",
     ["あ".encode("Shift_JIS"), "い".encode("Shift_JIS"), "é".encode("ISO-8859-1"), "う".encode("Shift_JIS"),
      "è".encode("ISO-8859-1"), "x".encode("Windows-31J"), "y".encode("Windows-31J"), "é", "é".b])

# The same bytes under another tag, for the tests that change an encoding and compare with what Ruby writes.
SJIS_TEXT = "日本語のテキストです".encode("Shift_JIS")
emit("str_force_euc_jp", "string", SJIS_TEXT.dup.force_encoding("EUC-JP"))
emit("str_force_binary", "string", SJIS_TEXT.b)
emit("str_force_utf8", "string", SJIS_TEXT.dup.force_encoding("UTF-8"))
emit("str_shift_jis_other", "string", "別のテキスト".encode("Shift_JIS"))
emit("str_shift_jis_short", "string", "日本語".encode("Shift_JIS"))

with_ivars = "テキスト".encode("Shift_JIS")
with_ivars.instance_variable_set(:@note, "メモ")
with_ivars.instance_variable_set(:@ノート, "ノート".encode("Shift_JIS"))
emit("str_with_ivars", "string", with_ivars)

array_with_ivar = [1, 2]
array_with_ivar.instance_variable_set(:@label, "ラベル")
emit("array_with_ivar", "string", array_with_ivar)

# Regexps: the source's encoding rides along as an `E`/`encoding` ivar.
{
  "ascii" => /abc+/i,
  "utf8" => /日本語|é/u,
  "binary" => Regexp.new("\xff\xfe".b, Regexp::FIXEDENCODING),
  "shift_jis" => Regexp.new("日本語".encode("Shift_JIS")),
  "euc_jp" => Regexp.new("テキスト".encode("EUC-JP")),
  "options" => /x y/mix,
  "noencoding" => /ab/n
}.each { |name, regexp| emit("regexp_#{name}", "regexp", regexp) }

# ---------------------------------------------------------------------------------------------------------
# Symbols as Ruby writes them: ASCII and UTF-8 ones, binary ones (no encoding - what Ruby 1.8 wrote for
# everything), and ones tagged with some other encoding.
# ---------------------------------------------------------------------------------------------------------
SYMBOLS = {
  "ascii" => :plain_symbol,
  "utf8" => :"日本語",
  "utf8_accent" => :"é",
  "binary_high" => "\xff\xfe".b.to_sym,
  "binary_sjis_bytes" => "アクター".encode("Shift_JIS").b.to_sym,
  "binary_with_nul" => "a\0\xffb".b.to_sym,
  "sjis_tagged" => "アクター".encode("Shift_JIS").to_sym,
  "euc_jp_tagged" => "アクター".encode("EUC-JP").to_sym
}
SYMBOLS.each { |name, symbol| emit("sym_#{name}", "symbol", symbol) }
emit("sym_links", "symbol", SYMBOLS.values + SYMBOLS.values + [Hash[SYMBOLS.map { |name, symbol| [symbol, name] }]])

# The same bytes as different symbols: one per encoding, as in Ruby. Windows-1252 and ISO-8859-1 accept any byte.
LATIN = "caf\xe9".b
latin1 = LATIN.dup.force_encoding("ISO-8859-1").to_sym
win1252 = LATIN.dup.force_encoding("Windows-1252").to_sym
binary = LATIN.dup.to_sym
emit("sym_latin1", "symbol", latin1)
emit("sym_win1252", "symbol", win1252)
emit("sym_latin1_binary", "symbol", binary)
emit("sym_same_bytes_three_encodings", "symbol", [latin1, win1252, binary, latin1, win1252, binary, { latin1 => win1252 }])
# Ruby shares each encoding's name, as an object, between the strings and symbols that use it.
emit("sym_and_str_share_encoding_names", "symbol",
     ["あ".encode("Shift_JIS"), "ア".encode("Shift_JIS").to_sym, "い".encode("Shift_JIS"), "ア".encode("EUC-JP").to_sym,
      "ア".encode("Shift_JIS").to_sym, "う".encode("EUC-JP"), "ア".encode("EUC-JP").to_sym, "é".encode("ISO-8859-1").to_sym])

# ---------------------------------------------------------------------------------------------------------
# Classes, ivars, structs ... with names that are not ASCII.
# ---------------------------------------------------------------------------------------------------------
emit("obj_plain", "object", Plain.new)
emit("obj_unicode_class", "object", Café.new)
emit("obj_struct_members", "object", Point.new("名前".encode("Shift_JIS"), 3))
emit("obj_user_defined_sjis", "object", Dumpy.new)
emit("obj_user_defined_binary", "object", DumpyBinary.new)
emit("obj_user_marshal", "object", Marshaled.new)
emit("obj_user_marshal_scalar", "object", ScalarMarshaled.new)
emit("obj_user_marshal_in_array", "object", [Marshaled.new, ScalarMarshaled.new(:sym), Marshaled.new, "after"])
emit("obj_user_marshal_array_ivar", "object", MarshaledArray.new)
emit("obj_extended_array_ivar", "object", [1].tap { |list| list.instance_variable_set(:@a, 2); list.extend(Extra) })
emit("obj_symbol_link_after_encoding", "object", [:é, Plain.new, Plain.new])
emit("obj_extended", "object", Object.new.extend(Módulo))
emit("obj_extended_twice", "object", Plain.new.extend(Extra).extend(Extra2))
emit("obj_exception", "object", RuntimeError.new("例外メッセージ"))
emit("obj_time", "object", Time.utc(2000, 1, 2, 3, 4, 5, 678_901))
emit("obj_range_rational_complex", "object", [(1..5), ("あ".."お"), Rational(3, 4), Complex(1, 2)])
emit("obj_hash_default_sjis", "object", Hash.new("デフォルト".encode("Shift_JIS")).merge(:"キー" => 1))

shared = "共有"
emit("obj_shared_values", "object", [shared, shared, Plain.new, :a, :a])

# ---------------------------------------------------------------------------------------------------------
# A larger, RPG-Maker-shaped document, and a big one.
# ---------------------------------------------------------------------------------------------------------
emit("complex_rpg_actors", "complex", (1..40).map { |id| RPG::Actor.new(id) })
emit("complex_rpg_map", "complex", RPG::Map.new)

records = Array.new(2_000) do |i|
  {
    :id => i,
    :"名前" => ["アイテム#{i}".encode("Shift_JIS"), "item#{i}", "アイテム#{i}"][i % 3],
    "price" => i * 1.25,
    :tags => [:"種類", :"c#{i % 5}"],
    :binary => [i].pack("N"),
    :nested => { :a => [i, { "b" => "ü" * (i % 4) }] }
  }
end
emit("complex_records", "complex", records)

# ---------------------------------------------------------------------------------------------------------
# Floats. Modern Ruby writes the shortest text that reads back exactly ("0.1"); Ruby 1.8 and 1.9.2 wrote digits, a
# NUL and the rest of the mantissa as raw bytes (marshal.c `save_mantissa`), mostly not UTF-8.
# ---------------------------------------------------------------------------------------------------------
MODERN_FLOATS = [0.0, -0.0, 1.5, 0.1, 1 / 3.0, Math::PI, 1e100, 1e-100, Float::MAX, Float::MIN, 5e-324,
                 123_456_789.123456789, -2.5e-7, Float::INFINITY, -Float::INFINITY, Float::NAN, 100.0, 1e22, 1e23]
emit("floats_modern", "float", MODERN_FLOATS)
emit("float_root", "float", 0.1)

DECIMAL_MANT = 53 - 16

# marshal.c `save_mantissa`: the mantissa bits the decimal text did not carry, 32 at a time, trailing zeros cut.
def save_mantissa(value)
  fraction, = Math.frexp(value.abs)
  scaled = fraction * (2**DECIMAL_MANT)
  rest = scaled - scaled.floor
  return "".b unless rest > 0

  out = "\0".b
  while rest > 0
    scaled = rest * (2**32)
    chunk = scaled.floor
    rest = scaled - chunk
    out << [chunk].pack("N")
  end
  out.chop! while out.getbyte(-1) == 0
  out
end

# Floats as Ruby 1.9.2 wrote them in the RPG Maker VX Ace data this was checked against: the shortest text that
# reads back exactly ("1.2", "0.02") or 17 digits ("0.80000000000000004"), then the 2-byte mantissa tail. Ruby
# itself still reads these (`load_mantissa`), which the generator checks.
LAYOUT_FLOATS = [1.2, 0.6, 0.8, 0.7, 0.08, 1.1, 0.9, 0.95, 0.2, 0.02, 0.33, 1.6, 98_765.4321, 0.1, 1 / 3.0, Math::PI,
                 123_456.789, -2.5e-7, 2.0**0.5, 4.35, 1e15 / 7, 3.0e-300 / 7, 1e-5 / 3]
LAYOUT_FLOATS.each_with_index do |value, index|
  tail = save_mantissa(value)
  [["shortest", Marshal.dump(value)[4..-1]], ["digits17", format("%.17g", value)]].each do |style, text|
    w = Wire.new
    w.float(text, tail)
    bytes = w.bytes
    loaded = Marshal.load(bytes)
    raise "Ruby cannot read back #{text} + #{Common.hex(tail)}: #{loaded}" unless loaded == value
    emit_raw("float_synthetic_#{style}_#{index}", "float_synthetic", bytes,
             "text" => text, "tail" => Common.hex(tail), "value_bits" => Common.hex([value].pack("E")))
  end
end

# Tails chosen to be invalid UTF-8 in every way that matters (lone continuation, truncated sequence, overlong,
# surrogate, 0xff, a NUL inside the tail) - or valid UTF-8, or empty. Real Ruby never wrote these; marshal-rs has to
# keep whatever it is given.
{
  "lone_continuation" => "\x80",
  "truncated_sequence" => "\xe2\x82",
  "overlong" => "\xc0\xaf",
  "surrogate" => "\xed\xa0\x80",
  "ff_bytes" => "\xff\xff\xff\xff",
  "nul_inside" => "\x00\xff\x00\x80",
  "valid_utf8" => "é日",
  "ascii_digits" => "12345",
  "empty_after_nul" => ""
}.each do |name, tail|
  tail = "\0".b + tail.b
  w = Wire.new
  w.float("1.5", tail)
  # Ruby 4 still accepts all of these (it folds the tail into extra precision); a failure here would mean the
  # fixture is not the kind of data it claims to be.
  Marshal.load(w.bytes)
  emit_raw("float_synthetic_tail_#{name}", "float_synthetic", w.bytes,
           "text" => "1.5", "tail" => Common.hex(tail), "value_bits" => Common.hex([1.5].pack("E")))
end

%w[inf -inf nan 0 -0].each do |text|
  tail = "\0\xff\xfe".b
  w = Wire.new
  w.float(text, tail)
  emit_raw("float_synthetic_special_#{text.tr('-', 'm')}", "float_synthetic", w.bytes, "text" => text, "tail" => Common.hex(tail))
end

# Many in one document, with object links back to earlier floats, and next to non-UTF-8 text.
w = Wire.new
w.array(LAYOUT_FLOATS.size * 2 + 1) do
  LAYOUT_FLOATS.each { |value| w.float(Marshal.dump(value)[4..-1], save_mantissa(value)) }
  LAYOUT_FLOATS.each_index { |index| w.link(1 + index) } # index 0 is the array itself
  w.float("2.5", "\0\xff\xfe".b)
end
emit_raw("float_synthetic_array_with_links", "float_synthetic_array", w.bytes, "count" => LAYOUT_FLOATS.size)

# ---------------------------------------------------------------------------------------------------------
# Every encoding this Ruby knows, as strings, symbols and regexps: its name is written once per document and
# linked afterwards, whatever it is called.
# ---------------------------------------------------------------------------------------------------------
def dumpable(*candidates)
  candidates.map do |make|
    begin
      value = make.call
      Marshal.load(Marshal.dump(value)) # Ruby has to be able to read what it wrote
      value
    rescue StandardError
      nil
    end
  end.compact
end

def text_in(encoding, text)
  text.encode(encoding)
rescue StandardError
  nil
end

ALL_ENCODINGS = Encoding.list
ALL_ENCODINGS.each do |encoding|
  name = encoding.name
  proper = text_in(encoding, "abc") || "abc".dup.force_encoding(encoding)
  rich = text_in(encoding, "ab é 日本") || text_in(encoding, "ab é") || proper
  raw = "\x81\xa0\xe9\xff\x00z".b.force_encoding(encoding)
  values = dumpable(
    -> { proper }, -> { rich }, -> { raw }, -> { "".b.force_encoding(encoding) },
    -> { proper.to_sym }, -> { rich.to_sym }, -> { raw.to_sym }, -> { "".b.force_encoding(encoding).to_sym },
    -> { Regexp.new(proper) }, -> { Regexp.new(rich) }, -> { Regexp.new(raw) },
    -> { Regexp.new(rich, Regexp::FIXEDENCODING) }
  )
  values = values + values # every name is used twice, so the second use is a link
  slug = name.gsub(/[^A-Za-z0-9]/, "_").downcase
  emit("enc_#{slug}", "encoding", values)
  emit("enc_one_#{slug}", "encoding_one", rich)
end

# The same bytes under every encoding, for the tests that re-tag a string and compare with what Ruby writes.
RETAG = "ab\xe9\x80z".b
ALL_ENCODINGS.each { |encoding| emit("retag_#{encoding.name.gsub(/[^A-Za-z0-9]/, '_').downcase}", "retag", RETAG.dup.force_encoding(encoding)) }

# One document using all of them, in a different order than they were first used.
mixed = ALL_ENCODINGS.reverse.map { |encoding| text_in(encoding, "x") || "x".dup.force_encoding(encoding) }
emit("enc_all_mixed", "encoding", mixed + mixed.reverse)

# ---------------------------------------------------------------------------------------------------------
# Core classes Ruby marshals itself: Time, Range, exceptions, Rational, Complex, Set, Date, ...
# ---------------------------------------------------------------------------------------------------------
# Times with a zone given by `in:` are in generate_ruby31.rb; the rest are plain UTC or fixed-offset times.
TIMES = {
  "utc" => Time.utc(2000, 1, 2, 3, 4, 5, 678_901),
  "utc_nsec" => Time.utc(2000, 1, 2, 3, 4, 5, Rational(123_456_789, 1000)),
  "offset_negative" => Time.new(1999, 12, 31, 23, 59, 59, -19_800),
  "local" => Time.at(1_700_000_000),
  "far_future" => Time.utc(9999, 12, 31, 23, 59, 59),
  "year_1" => Time.utc(1, 1, 1)
}
# Ruby 1.9.3 cannot marshal the year 1 ("year too big"): such a time is skipped there, as the ranges below are.
TIMES.each do |name, time|
  begin
    emit("core_time_#{name}", "core", time)
  rescue ArgumentError => error
    warn "skipped core_time_#{name}: #{error.message}"
  end
end

# Endless and beginless ranges are `Range.new` here: older Rubies reject the literal forms, and the rescue skips
# the ranges they cannot build.
{
  "int" => -> { 1..5 }, "exclusive" => -> { 1...5 }, "endless" => -> { Range.new(1, nil) },
  "beginless" => -> { Range.new(nil, 5) }, "everything" => -> { Range.new(nil, nil) }, "string" => -> { "あ".."お" },
  "float" => -> { 1.5..2.5 }, "negative" => -> { -5..-1 }
}.each do |name, make|
  begin
    emit("core_range_#{name}", "core", make.call)
  rescue StandardError => error
    warn "skipped core_range_#{name}: #{error.message}"
  end
end
shared_range = 1..3
emit("core_range_shared", "core", [shared_range, shared_range, 4...6])

emit("core_exc_runtime", "core", RuntimeError.new("メッセージ"))
emit("core_exc_argument", "core", ArgumentError.new("é".encode("ISO-8859-1")))
emit("core_exc_backtrace", "core", RuntimeError.new("bt").tap { |e| e.set_backtrace(["a.rb:1:in `x'", "b.rb:2"]) })
emit("core_exc_custom", "core", MyError.new)
emit("core_exc_custom_many", "core", [MyError.new("one", 1), MyError.new("two", [2]), RuntimeError.new("three")])
record("core_exc_no_message", Marshal.dump(RuntimeError.new), "core", nil)
record("core_exc_raised", Marshal.dump(begin; raise "boom"; rescue => e; e; end), "core", nil)
record("core_exc_cause", Marshal.dump(begin; begin; raise "a"; rescue; raise "b"; end; rescue => e; e; end), "core", nil)
{
  "system_exit" => -> { SystemExit.new(3, "bye") }, "name_error" => -> { NameError.new("m", :foo) },
  "key_error" => -> { KeyError.new("k", receiver: {}, key: 1) }, "no_method" => -> { NoMethodError.new("m", :bar) },
  "stop_iteration" => -> { StopIteration.new("s") }, "enoent" => -> { Errno::ENOENT.new("path") },
  "frozen" => -> { FrozenError.new("f", receiver: "x") }, "interrupt" => -> { Interrupt.new("int") }
}.each do |name, make|
  begin
    record("core_exc_#{name}", Marshal.dump(make.call), "core", nil)
  rescue StandardError => problem
    warn "skipped core_exc_#{name}: #{problem.message}"
  end
end

emit("core_rational", "core", [Rational(3, 4), Rational(-1, 3), Rational(0), Rational(2**70, 3), Rational(5, 1)])
emit("core_complex", "core", [Complex(1, 2), Complex(1.5, -2.5), Complex(0, 0), Complex(2**70, 1), Complex(Rational(1, 2), 3)])
emit("core_date", "core", [Date.new(2000, 1, 2), Date.new(1582, 10, 15), Date.new(-4712, 1, 1), Date.new(9999, 12, 31)])
emit("core_datetime", "core", [DateTime.new(2000, 1, 2, 3, 4, 5), DateTime.new(2000, 1, 2, 3, 4, 5, "+09:00")])
emit("core_random", "core", Random.new(42))
# (Not one twice: Ruby itself loads the link to a repeated Encoding as the String its data was.)
emit("core_encoding_objects", "core", [Encoding::UTF_8, Encoding::Shift_JIS, Encoding::BINARY, Encoding::EUC_JP, Encoding::US_ASCII])
if defined?(BigDecimal)
  emit("core_bigdecimal", "core", [BigDecimal("1.5"), BigDecimal("-0.001"), BigDecimal("NaN"), BigDecimal("Infinity"), BigDecimal("1e1000")])
end
emit("core_set", "core", [Set.new, Set[1, 2, 3], Set["a", :b, nil], Set[Set[1]]])
emit("core_ostruct", "core", OpenStruct.new(:a => 1, :"名前" => "x")) if defined?(OpenStruct)
emit("core_hash_identity", "core", [{ a: 1 }.compare_by_identity, Hash.new(5).compare_by_identity.tap { |h| h["x".dup] = 1 }])
emit("core_struct_tms", "core", Process.times)
emit("core_object_bare", "core", [Object.new, Object.new])
emit("core_class_refs", "core",
     [Object, BasicObject, Kernel, Comparable, Enumerable, Integer, Float, String, Struct, Time, Range, Process, Math,
      Errno::ENOENT, Process.times.class, Encoding, Marshal])
emit("core_symbols_odd", "core", [:+, :[]=, :"with space", :"9", :"", :"a\0b", :a?, :b!, :C, :@iv, :@@cv, :$g, :"é", :"\u0000"])

# Wrappers stacked on one value: a subclass, `extend`, instance variables and an encoding.
emit("combo_plain_values", "core", ["plain".tap { |s| s.instance_variable_set(:@i, 1) }, [1].tap { |a| a.instance_variable_set(:@j, [2]) },
                                    { 1 => 2 }.tap { |h| h.instance_variable_set(:@k, "v") }, Regexp.new("r").tap { |r| r.instance_variable_set(:@l, nil) }])
emit("combo_struct_ivar", "core", MutStruct.new(1, 2, 3, 4, 5).tap { |s| s.instance_variable_set(:@note, "メモ") })

# Sizes: the lengths where Marshal's own integer encoding changes its width, and large values.
lengths = [0, 1, 121, 122, 123, 124, 253, 254, 255, 256, 257, 65_533, 65_534, 65_535, 65_536, 65_537]
emit("size_strings", "size", lengths.map { |n| "x" * n })
emit("size_binary_strings", "size", lengths.map { |n| ("\xff" * n).b })
emit("size_arrays", "size", lengths.map { |n| Array.new(n) })
emit("size_hashes", "size", lengths.first(12).map { |n| Hash[(0...n).map { |i| [i, i] }] })
emit("size_symbols", "size", lengths.first(14).map { |n| ("s" * [n, 1].max).to_sym })
emit("size_bignum_huge", "size", [2**20_000, -(2**20_000)])
emit("size_array_70k", "size", (0...70_000).to_a)
emit("size_string_200k", "size", "x" * 200_000)
emit("size_hash_5k", "size", Hash[(0...5_000).map { |i| ["key#{i}", i] }])

# ---------------------------------------------------------------------------------------------------------
# Changes: a value, Ruby's version of it after the change, and (in the Rust tests) the API calls that make the
# same change. The tests require the dump of the changed arena to be exactly what Ruby wrote for the changed value.
# ---------------------------------------------------------------------------------------------------------
def change(name, before)
  after = yield Marshal.load(Marshal.dump(before))
  emit("mut_#{name}_before", "mutation", before)
  emit("mut_#{name}_after", "mutation", after)
end

ARRAY = lambda { [1, "two", :three, 4.5, nil, [5]] }
change("array_value", ARRAY.call) { |a| a[1] = [7, 8]; a }
change("array_string", ARRAY.call) { |a| a[1] = "ünï"; a }
change("array_bytes", ARRAY.call) { |a| a[1] = "\xff\x00".b; a }
change("array_symbol", ARRAY.call) { |a| a[2] = :"シンボル"; a }
change("array_fixnum", ARRAY.call) { |a| a[0] = -70_000; a }
change("array_bool", ARRAY.call) { |a| a[4] = true; a }
change("array_link", ARRAY.call) { |a| a[3] = a[1]; a }
change("array_cycle", ARRAY.call) { |a| a[5] = a; a }
change("array_to_symbol_seen", ARRAY.call) { |a| a[0] = :three; a }

HASH = lambda { Hash.new("d").merge(:a => 1, "b" => "two", 3 => [3]) }
change("hash_key", HASH.call) do |h|
  n = Hash.new(h.default)
  h.each { |k, v| n[k == :a ? :z : k] = v }
  n
end
change("hash_value", HASH.call) { |h| h["b"] = "new"; h }
change("hash_default", HASH.call) { |h| h.default = "other"; h }

change("member_value", Mut.new) { |o| o.instance_variable_set(:@d, { 1 => 2 }); o }
change("member_string", Mut.new) { |o| o.instance_variable_set(:@b, "ß"); o }
change("member_bytes", Mut.new) { |o| o.instance_variable_set(:@b, "\xff".b); o }
change("member_symbol", Mut.new) { |o| o.instance_variable_set(:@c, :"記号"); o }
change("member_fixnum", Mut.new) { |o| o.instance_variable_set(:@a, 99); o }
change("member_bool", Mut.new) { |o| o.instance_variable_set(:@e, true); o }
change("struct_member_value", MutStruct.new(1, "two", :three, [4], nil)) { |s| s.d = { 1 => 2 }; s }
change("struct_member_string", MutStruct.new(1, "two", :three, [4], nil)) { |s| s.b = "ß"; s }
change("struct_member_fixnum", MutStruct.new(1, "two", :three, [4], nil)) { |s| s.a = 99; s }
change("struct_member_symbol", MutStruct.new(1, "two", :three, [4], nil)) { |s| s.c = :"記号"; s }

change("bytes_content", "\x00\x01".b) { |s| s.replace("\xff\xfe\xfd".b) }
change("bytes_content_shared", (x = "ab".b; [x, x, [x]])) { |a| a[0].replace("cdef".b); a }
change("string_text_shared", (x = "abc"; [x, x, [x]])) { |a| a[0].replace("änder"); a }
change("string_text_empty", "abc") { |s| s.replace("") }
change("fixnum_value", 5) { |_| 70_000 }
change("fixnum_negative", 5) { |_| -1 }
change("take_bytes", ["ab".b, "keep"]) { |a| a[0] = "".b; a }
change("take_regexp", [/abc/i, 1]) { |a| a[0] = Regexp.new("", Regexp::IGNORECASE); a }

change("user_defined_binary", [1, 2, 3, 0xff, 0xfe].pack("C*")) { |_| DumpyBinary.new }
change("user_defined_sjis", "ダンプ".encode("Shift_JIS")) { |_| Dumpy.new }

change("extend_object", Plain.new) { |o| o.extend(Módulo) }
change("extend_twice", Plain.new) { |o| o.extend(Extra).extend(Extra2) }
change("extend_array", [1]) { |a| a.extend(Extra) }
change("extend_string", "s") { |s| s.extend(Módulo) }

change("ivar_string", "abc") { |s| s.instance_variable_set(:@note, "メモ"); s }
change("ivar_string_sjis", "テキスト".encode("Shift_JIS")) { |s| s.instance_variable_set(:@note, "メモ"); s }
change("ivar_array", [1, 2]) { |a| a.instance_variable_set(:@label, "ラベル"); a }
change("ivar_hash", { 1 => 2 }) { |h| h.instance_variable_set(:@label, :sym); h }
change("ivar_object", Plain.new) { |o| o.instance_variable_set(:@z, [1]); o }
change("ivar_two", "abc") { |s| s.instance_variable_set(:@a, 1); s.instance_variable_set(:@b, "x"); s }
change("ivar_shared_value", (x = "v"; [x, "t"])) { |a| a[1].instance_variable_set(:@v, a[0]); a }
change("root_element", [1, "a"]) { |a| a[1] }

# ---------------------------------------------------------------------------------------------------------
# Names that carry an encoding of their own. Ruby never writes these (class and member names are UTF-8 or bare),
# but it reads them, and numbers the encoding name (a string object) at a point that depends on what the symbol
# names: before an `extend`ed module's or a subclass's object and before a `marshal_dump` / `_dump` value, after
# an `Object` or `Struct`. Links after the name only mean the right thing when that is respected. Each stream is
# built byte by byte, checked by loading it in Ruby, and described by what Ruby loaded.
# ---------------------------------------------------------------------------------------------------------
class Wire
  def symbols; (@symbols ||= {}); end
  def names; (@names ||= {}); end
  def objects; (@objects ||= 0); end
  def object!; (@objects = objects + 1); end

  # An array is an object like any other.
  def list(count, &block); (object!; array(count, &block)); end

  # A symbol, written the way Ruby does: in full the first time, `;<index>` after, tagged with `encoding` (whose name
  # is an object, written once and linked afterwards) when given.
  def sym(name, encoding = nil)
    return tag(";").long(symbols[name]) if symbols.key?(name)

    symbols[name] = symbols.size
    if encoding
      tag("I:").chunk(name)
      long(1)
      sym("encoding")
      if names.key?(encoding)
        link(names[encoding])
      else
        names[encoding] = objects
        object!
        tag('"').chunk(encoding)
      end
    else
      tag(":").chunk(name)
    end
    self
  end
end

def tagged_names(name)
  w = Wire.new
  yield w
  bytes = w.bytes
  loaded = Marshal.load(bytes) # Ruby has to accept it
  record(name, bytes, "tagged_names", Describe.desc(loaded))
end

SJIS = "Shift_JIS"
tagged_names("tagged_name_object") { |w| w.list(3) { w.object!; w.tag("o"); w.sym("Plain", SJIS); w.long(0); w.link(1); w.link(2) } }
tagged_names("tagged_name_struct") do |w|
  w.list(3) do
    w.object!; w.tag("S"); w.sym("MutStruct", SJIS); w.long(5)
    %w[a b c d e].each_with_index { |member, index| w.sym(member); w.tag("i"); w.long(index + 1) }
    w.link(1); w.link(2)
  end
end
tagged_names("tagged_name_user_marshal") do |w|
  w.list(3) do
    w.tag("U"); w.sym("Rational", SJIS); w.object!
    w.list(2) { w.tag("i"); w.long(1); w.tag("i"); w.long(2) }
    w.link(1); w.link(2)
  end
end
tagged_names("tagged_name_user_defined_with_ivar") do |w|
  w.list(4) do
    w.tag("Iu"); w.sym("DumpyBinary", SJIS); w.chunk("abc"); w.long(1); w.sym("a")
    w.object!; w.tag('"'); w.chunk("x") # the ivar's value, numbered before the object it belongs to
    w.object!
    w.link(1); w.link(2); w.link(3)
  end
end
tagged_names("tagged_name_extended") do |w|
  w.list(3) { w.tag("e"); w.sym("Extra", SJIS); w.object!; w.tag("o"); w.sym("Plain"); w.long(0); w.link(1); w.link(2) }
end
tagged_names("tagged_name_subclass") do |w|
  w.list(3) { w.tag("C"); w.sym("PlainString", SJIS); w.object!; w.tag('"'); w.chunk("x"); w.link(1); w.link(2) }
end
tagged_names("tagged_member_name") do |w|
  w.list(3) { w.object!; w.tag("o"); w.sym("Plain"); w.long(1); w.sym("@a", SJIS); w.tag("i"); w.long(5); w.link(1); w.link(2) }
end
tagged_names("tagged_names_share_the_encoding_name") do |w|
  w.list(5) do
    w.object!; w.tag("o"); w.sym("Plain", SJIS); w.long(0)
    w.object!; w.tag("o"); w.sym("Mut", SJIS); w.long(0)
    w.link(1); w.link(2); w.link(3)
  end
end

# Fixtures that need the classes of a newer Ruby (see classes.rb), which only the Rubies that have them load.
require_relative "generate_ruby26" if RUBY_VERSION >= "2.6"
require_relative "generate_ruby31" if RUBY_VERSION >= "3.1"
require_relative "generate_ruby32" if RUBY_VERSION >= "3.2"

File.write(File.join(OUT, "manifest.json"), MiniJSON.dump("fixtures" => FIXTURES))
puts "wrote #{FIXTURES.size} fixtures to #{OUT}"
