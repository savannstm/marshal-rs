# Writes the Ruby 1.8.7 fixtures: what RPG Maker XP's Ruby really produced. Run with `-Ks`:
#
#   ruby187 -Ks tests/fixtures/generate_ruby18.rb <output dir>
#
# Ruby 1.8 has no string encodings, so none of these carry one: every string, symbol and name is just bytes
# (Shift_JIS here), and every float is 17 digits followed, when the double has more mantissa bits than the text
# carries, by a NUL and two or more raw mantissa bytes (marshal.c `save_mantissa`). Ruby 4 can read such floats and
# cannot write them.

require "fileutils"
require "stringio"
require File.expand_path("ruby_common", File.dirname(__FILE__))
require File.expand_path("classes18", File.dirname(__FILE__))

OUT = ARGV[0] or abort("usage: ruby -Ks generate_ruby18.rb <output dir>")
FileUtils.mkdir_p(OUT)

FIXTURES = []

def emit(name, group, object, extra = {})
  raise "duplicate fixture #{name}" if FIXTURES.any? { |fixture| fixture["name"] == name }
  bytes = Marshal.dump(object)
  File.open(File.join(OUT, name + ".bin"), "wb") { |file| file.write(bytes) }
  FIXTURES << { "name" => name, "group" => group, "desc" => Describe.desc(object) }.merge(extra)
end

# ---------------------------------------------------------------------------------------------------------
# Floats
# ---------------------------------------------------------------------------------------------------------
ZERO = 0.0
FLOATS = [
  0.1, 0.2, 0.3, 0.6, 0.7, 0.8, 0.9, 1.1, 1.2, 1.6, 0.95, 0.08, 0.02, 0.33, 2.675, 4.35,
  1.0 / 3, 2.0 / 3, Math::PI, Math::E, 2.0**0.5, 3.14159, 123456.789, 98765.4321, 12345678.9, 0.0001, 1e-5,
  1e22, 1e100, 1e300, 1e-300, 5e-324, 1.7976931348623157e308, -0.1, -1.0 / 3, -2.5e-7,
  # no mantissa bits beyond the text: Ruby writes these without a tail
  0.0, -0.0, 1.0, 1.5, 0.5, 0.25, 100.0, 2.0, 1024.5, 3.0e10, -7.0,
  # not numbers
  1.0 / ZERO, -1.0 / ZERO, ZERO / ZERO
]

emit("r187_floats", "float18", FLOATS)
FLOATS.each_with_index { |value, index| emit("r187_float_#{index}", "float18", value) }
emit("r187_floats_in_hash", "float18", { 0.1 => 0.2, 1.0 / 3 => [2.0 / 3, 0.7], :sym => 1.2, "str" => Math::PI })
emit("r187_floats_shared", "float18", (x = 0.1; [x, x, 0.1, [x]]))

# ---------------------------------------------------------------------------------------------------------
# Strings, symbols and names that are bytes
# ---------------------------------------------------------------------------------------------------------
emit("r187_str_ascii", "string18", "plain ascii")
emit("r187_str_empty", "string18", "")
emit("r187_str_sjis", "string18", Sjis::JAPANESE + Sjis::TEXT)
emit("r187_str_binary", "string18", "\x00\x01\xfe\xff binary")
emit("r187_str_nul", "string18", "a\0b\0\0c")
emit("r187_str_long", "string18", Sjis::TEXT * 20_000)
emit("r187_strs_shared", "string18", (s = Sjis::TEXT; [s, s, Sjis::TEXT, [s]]))

emit("r187_sym_ascii", "symbol18", :plain)
emit("r187_sym_sjis", "symbol18", Sjis::ACTOR.to_sym)
emit("r187_sym_high_bytes", "symbol18", "\xff\xfe".to_sym)
emit("r187_sym_links", "symbol18", [Sjis::ACTOR.to_sym, :a, Sjis::ACTOR.to_sym, "\xff\xfe".to_sym, :a, "\xff\xfe".to_sym])
emit("r187_sym_hash_keys", "symbol18",
     [{ Sjis::NAME.to_sym => 1, Sjis::JAPANESE.to_sym => Sjis::WARRIOR }, { Sjis::NAME.to_sym => 2, Sjis::JAPANESE.to_sym => Sjis::TEXT }])

emit("r187_regexp_ascii", "regexp18", /abc+/i)
emit("r187_regexp_sjis", "regexp18", Regexp.new(Sjis::JAPANESE, nil, "s"))
emit("r187_regexp_binary", "regexp18", Regexp.new("\xff\xfe", nil, "n"))

# ---------------------------------------------------------------------------------------------------------
# Objects whose class, ivar and member names are Shift_JIS bytes
# ---------------------------------------------------------------------------------------------------------
emit("r187_obj_plain", "object18", $plain_class.new)
emit("r187_obj_sjis_names", "object18", $actor_class.new(1))
emit("r187_obj_sjis_names_array", "object18", (1..10).map { |id| $actor_class.new(id) })
emit("r187_obj_struct", "object18", $struct_class.new(Sjis::TEXT, 2, [3.5]))
emit("r187_obj_string_sub", "object18", $string_sub.new(Sjis::TEXT))
emit("r187_obj_array_sub", "object18", $array_sub.new([1, Sjis::TEXT]))
emit("r187_obj_hash_sub", "object18", $hash_sub.new.merge(Sjis::KEY => 1))
emit("r187_obj_extended", "object18", $plain_class.new.extend($extension_module))
emit("r187_obj_extended_twice", "object18", $plain_class.new.extend($extra_module).extend($extension_module))
emit("r187_obj_user_defined", "object18", $dump_class.new(Sjis::DUMP + "\x00\xff"))
emit("r187_obj_user_marshal", "object18", $marshal_class.new([Sjis::TEXT, :sym, 1.5, [Sjis::KEY.to_sym]]))
emit("r187_obj_class_refs", "object18", [$actor_class, $extension_module, String, Comparable, $struct_class])
emit("r187_obj_hash_default", "object18", Hash.new(Sjis::TEXT).merge(Sjis::KEY.to_sym => 1))
emit("r187_obj_bignums", "object18", [2**64, -(2**100), 2**31, -(2**31) - 1, 2**1000])

cycle = [1, "x", :a]
cycle << cycle
emit("r187_obj_cycle", "object18", cycle)

# ---------------------------------------------------------------------------------------------------------
# An RPG Maker-shaped document (actors with Table/Color blobs, floats everywhere) and a big one
# ---------------------------------------------------------------------------------------------------------
emit("r187_complex_actors", "complex18", (1..40).map { |id| RPG::Actor.new(id) })
emit("r187_complex_map", "complex18", RPG::Map.new)

records = (0...2000).map do |i|
  { :id => i, Sjis::NAME.to_sym => [Sjis::HERO + i.to_s, "item" + i.to_s][i % 2], "price" => i * 1.25 + 0.1,
    :tags => [Sjis::KIND.to_sym, :"c#{i % 5}"], :binary => [i].pack("N"), :nested => { :a => [i, { "b" => Sjis::TEXT * (i % 4) }] } }
end
emit("r187_complex_records", "complex18", records)

# ---------------------------------------------------------------------------------------------------------
# Core classes Ruby 1.8.7 marshals itself
# ---------------------------------------------------------------------------------------------------------
emit("r187_core_time", "core18", [Time.at(0).utc, Time.at(1_700_000_000), Time.utc(2000, 1, 2, 3, 4, 5, 678_901), Time.gm(1970)])
t = Time.utc(2001, 2, 3)
emit("r187_core_time_shared", "core18", [t, t, Time.at(1_700_000_000)])
emit("r187_core_range", "core18", [1..5, 1...5, "a".."e", 1.5..2.5, -5..-1, Sjis::TEXT..Sjis::TEXT + "z"])
r = 1..3
emit("r187_core_range_shared", "core18", [r, r, 4...6])
emit("r187_core_exceptions", "core18",
     [RuntimeError.new("msg"), ArgumentError.new(Sjis::TEXT), StandardError.new("x").tap { |e| e.set_backtrace(["a.rb:1", "b.rb:2"]) }])
emit("r187_core_exception_raised", "core18", (begin; raise "boom"; rescue => e; e; end))
emit("r187_core_set", "core18", [Set.new, Set.new([1, 2, 3]), Set.new([Sjis::TEXT, :a])])
emit("r187_core_ostruct", "core18", OpenStruct.new(:a => 1, :b => Sjis::TEXT))
emit("r187_core_tms", "core18", Process.times)
emit("r187_core_class_refs", "core18", [Object, Kernel, Comparable, Integer, String, Time, Range, Process, Math, Struct])
emit("r187_core_symbols_odd", "core18", [:+, :[]=, :"with space", :"9", :a?, :b!, :C, :@iv, :@@cv, :$g, :"\xff"])
emit("r187_core_object", "core18", [Object.new, Object.new])
begin
  emit("r187_core_date", "core18", [Date.new(2000, 1, 2), Date.new(1582, 10, 15)])
rescue StandardError, LoadError => error
  puts "skipped Date: #{error.message}"
end
begin
  require "bigdecimal"
  emit("r187_core_bigdecimal", "core18", [BigDecimal("1.5"), BigDecimal("-0.001"), BigDecimal("1e100")])
rescue LoadError, StandardError => error
  puts "skipped BigDecimal: #{error.message}"
end

# Where Marshal's own integer encoding changes its width.
lengths = [0, 1, 121, 122, 123, 124, 253, 254, 255, 256, 257, 65_533, 65_534, 65_535, 65_536, 65_537]
emit("r187_size_strings", "size18", lengths.map { |n| "x" * n })
emit("r187_size_arrays", "size18", lengths.map { |n| Array.new(n) })
emit("r187_size_hashes", "size18", lengths.first(12).map { |n| h = {}; n.times { |i| h[i] = i }; h })
emit("r187_size_array_70k", "size18", (0...70_000).to_a)

File.open(File.join(OUT, "manifest.json"), "w") { |file| file.write(MiniJSON.dump("fixtures" => FIXTURES)) }
puts "wrote #{FIXTURES.size} fixtures to #{OUT}"
