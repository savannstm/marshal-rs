# Dumps one stream per value into the output directory, with the Ruby running it. Ruby 1.8 syntax, ASCII source:
# non-ASCII text is written as escapes, so every version reads the file the same way.
#
#   ruby matrix.rb <output dir>

require File.join(File.dirname(__FILE__), "matrix_classes")

OUT = ARGV[0] or abort("usage: ruby matrix.rb <output dir>")
Dir.mkdir(OUT) unless File.directory?(OUT)

VALUES = []

def add(name, value)
  VALUES << [name, value]
end

shared = "shared text"
hash_default = Hash.new(0)
hash_default["k"] = 1
extended = Object.new
extended.extend(MatrixMixin)

add("nil", nil)
add("true", true)
add("false", false)
add("fixnum_zero", 0)
add("fixnum_122", 122)
add("fixnum_neg_123", -123)
add("fixnum_256", 256)
add("fixnum_max_small", 2**30 - 1)
add("bignum_pos", 2**70)
add("bignum_neg", -(2**70))
add("float_tenth", 0.1)
add("float_neg_zero", -0.0)
add("float_huge", 1.0e100)
add("float_inf", 1.0 / 0.0)
add("string_ascii", "plain")
add("string_empty", "")
add("string_binary", [0, 127, 128, 255].pack("C*"))
add("string_utf8", "caf\xC3\xA9")
add("symbol_ascii", :plain)
add("symbol_utf8", "caf\xC3\xA9".to_sym)
add("array_nested", [1, [2, [3, "x"]], nil])
add("hash_plain", {"a" => 1, :b => [2]})
add("hash_default", hash_default)
add("regexp", /ab+c/i)
add("range_inclusive", 1..5)
add("range_exclusive", 1...5)
add("struct", MatrixPair.new(1, "two"))
add("object", MatrixPoint.new(3, 4))
add("user_marshal", MatrixWrapped.new("wrapped"))
add("user_defined", MatrixDumped.new("dumped"))
add("extended_object", extended)
add("exception", Exception.new("boom"))
add("class_ref", String)
add("module_ref", Comparable)
add("shared_references", [shared, shared])

VALUES.each do |name, value|
  File.open(File.join(OUT, name + ".bin"), "wb") { |file| file.write(Marshal.dump(value)) }
end
puts "dumped #{VALUES.size} values"
