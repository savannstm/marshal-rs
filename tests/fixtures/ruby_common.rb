# Shared by the fixture generators and the verifier. The syntax is Ruby 1.8-compatible on purpose: this file also
# runs on Ruby 1.8.7 (see generate_ruby18.rb), the interpreter RPG Maker XP's data was written by.

module Common
  def self.hex(string)
    string.unpack("H*")[0]
  end

  # The bytes of `string`, whatever its encoding.
  def self.raw(string)
    string.respond_to?(:b) ? string.b : string.dup
  end

  def self.encoding_of(string)
    string.respond_to?(:encoding) ? string.encoding.name : "ASCII-8BIT"
  end

  # What Ruby calls the encoding of a symbol; Ruby 1.8 has none, so an ASCII symbol is US-ASCII and any other binary.
  def self.symbol_encoding_of(symbol)
    text = symbol.to_s
    return text.encoding.name if text.respond_to?(:encoding)
    text.unpack("C*").any? { |byte| byte > 127 } ? "ASCII-8BIT" : "US-ASCII"
  end

  def self.class_hex(klass)
    hex(klass.name)
  end
end

# The JSON the manifest needs - strings (ASCII only), integers, nil, booleans, arrays and string-keyed hashes -
# so that no `json` library is required on Ruby 1.8.7.
module MiniJSON
  def self.dump(value)
    case value
    when nil then "null"
    when true then "true"
    when false then "false"
    when Integer then value.to_s
    when String
      raise "not plain ASCII: #{value.inspect}" unless value =~ /\A[\x20-\x7e]*\z/
      '"' + value.gsub(/["\\]/) { |match| "\\" + match } + '"'
    when Symbol then dump(value.to_s)
    when Array then "[" + value.map { |element| dump(element) }.join(",") + "]"
    # Sorted: Ruby 1.8's hashes do not keep insertion order, and the matrix compares the text of two runs.
    when Hash then "{" + value.keys.sort_by { |key| key.to_s }.map { |key| dump(key.to_s) + ":" + dump(value[key]) }.join(",") + "}"
    else raise "cannot dump a #{value.class}"
    end
  end
end

# A description of a Ruby value that marshal-rs's tests compare their `Arena` with: the Ruby side says what is
# in the value, the Rust side checks it found the same. `desc` returns nil for anything it does not model
# (Time, Range, exceptions, cycles, ...); those fixtures are still round-tripped, just not inspected.
module Describe
  class Unknown < StandardError; end

  MAX_DEPTH = 40 # the manifest is read with serde_json, which stops at 128 levels of JSON
  # Ruby before 3 writes an identity hash as a plain hash: only a Ruby whose stream says so has the flag to describe.
  IDENTITY_IN_STREAM = {}.respond_to?(:compare_by_identity) && Marshal.dump({}.compare_by_identity).include?("Hash")
  @plain = []

  # The user classes whose instances are described by their ivars (or `_dump` / `marshal_dump`).
  def self.register(*klasses)
    @plain.concat(klasses)
  end

  def self.desc(object)
    describe(object, 0)
  rescue Unknown
    nil
  end

  # Exceptions that carry nothing but a message and a backtrace; the others (SystemExit, KeyError, ...) have more
  # state of their own and are only round-tripped. Exception itself is one of them.
  SIMPLE_EXCEPTIONS = [Exception, StandardError, RuntimeError, ArgumentError, TypeError, IOError, RangeError, IndexError]

  # Core classes Ruby marshals itself (Time, Range, exceptions, Rational, ...), described through what lands in
  # the stream. Their data is private to the class, so it is asked for the way `Marshal` asks. nil: not one of those.
  def self.describe_core(o, d)
    name = o.class.name
    ivars = lambda { |pairs| pairs.map { |key, value| [Common.hex(key), describe(value, d)] } }
    case o
    when Time
      raise Unknown unless o.class == Time
      data = o.send(:_dump)
      nsec = o.respond_to?(:nsec) ? o.nsec : o.usec * 1000
      { "t" => "udump", "class" => Common.hex("Time"), "enc" => Common.encoding_of(data), "hex" => Common.hex(data),
        "time" => [o.to_i, nsec, o.utc_offset, o.utc? ? 1 : 0, Common.hex(o.zone.to_s)] }
    when Range
      raise Unknown unless o.class == Range
      { "t" => "obj", "class" => Common.hex("Range"),
        "ivars" => ivars.call(RUBY_VERSION < "1.9" ? [["excl", o.exclude_end?], ["end", o.end], ["begin", o.begin]] :
                                                     [["excl", o.exclude_end?], ["begin", o.begin], ["end", o.end]]) }
    when Exception
      raise Unknown unless SIMPLE_EXCEPTIONS.include?(o.class) || @plain.include?(o.class)
      # A loaded exception keeps its backtrace as strings, and only those are described: Ruby 2.7.8 crashes (a
      # segmentation fault) when asked for `backtrace_locations` of one.
      # The order each Ruby writes them in, as the fixtures show: 1.8 writes the backtrace first.
      pairs = if RUBY_VERSION < "1.9"
                [["bt", o.backtrace], ["mesg", o.message]]
              else
                [["mesg", o.message], ["bt", o.backtrace]]
              end
      pairs << ["cause", o.cause] if o.respond_to?(:cause) && o.cause
      o.instance_variables.each { |ivar| pairs << [ivar.to_s, o.instance_variable_get(ivar)] }
      { "t" => "obj", "class" => Common.class_hex(o.class), "ivars" => ivars.call(pairs) }
    else
      if o.class.respond_to?(:members) && o.respond_to?(:to_h) # Data (Ruby 3.2+); Struct was matched before
        raise Unknown unless @plain.include?(o.class)
        values = o.to_h
        return { "t" => "struct", "class" => Common.class_hex(o.class),
                 "members" => o.class.members.map { |member| [Common.hex(member.to_s), describe(values[member], d)] } }
      end
      case name
      when "Set"
        # Ruby 4 hides the one instance variable of its built-in Set; it is written as `@hash`, element => true.
        return nil unless o.class.name == "Set" && o.instance_variables.empty?
        hash = Hash.new(false)
        o.each { |element| hash[element] = true }
        { "t" => "obj", "class" => Common.hex("Set"), "ivars" => ivars.call([["@hash", hash]]) }
      when "Rational", "Complex", "Date", "DateTime", "Random"
        return nil unless o.respond_to?(:marshal_dump, true)
        { "t" => "umarshal", "class" => Common.hex(name), "v" => describe(o.send(:marshal_dump), d) }
      when "Encoding", "BigDecimal"
        data = o.send(:_dump)
        { "t" => "udump", "class" => Common.hex(name), "enc" => Common.encoding_of(data), "hex" => Common.hex(data) }
      end
    end
  end

  # The modules a value was `extend`ed with, nearest first - the order Marshal writes them in.
  def self.extended_modules(o)
    return [] if o.nil? || o == true || o == false || o.is_a?(Integer) || o.is_a?(Symbol) || o.is_a?(Float)
    singleton = (class << o; self; end)
    (singleton.included_modules - o.class.included_modules).map { |mod| Common.hex(mod.name) }
  rescue TypeError
    []
  end

  # What a value carries besides what its kind says: instance variables (in the order they are written) and `extend`s.
  def self.extras(o, d)
    result = {}
    names = o.instance_variables
    unless names.empty?
      result["ivars"] = names.map { |name| [Common.hex(name.to_s), describe(o.instance_variable_get(name), d)] }
    end
    ext = extended_modules(o)
    result["ext"] = ext unless ext.empty?
    result
  end

  def self.describe(o, depth)
    raise Unknown if depth > MAX_DEPTH
    d = depth + 1
    case o
    when nil then { "t" => "nil" }
    when true then { "t" => "true" }
    when false then { "t" => "false" }
    when Integer
      if o >= -(2**30) && o < 2**30
        { "t" => "int", "v" => o }
      else
        { "t" => "big", "v" => o.to_s }
      end
    when Float
      result = { "t" => "float", "hex" => Common.hex(Marshal.dump(o)[4..-1]), "bits" => Common.hex([o].pack("E")) }
      result["nan"] = true if o != o
      result
    when Symbol then { "t" => "sym", "hex" => Common.hex(o.to_s), "enc" => Common.symbol_encoding_of(o) }
    when String
      result = { "t" => "str", "enc" => Common.encoding_of(o), "hex" => Common.hex(o) }
      result["class"] = Common.class_hex(o.class) unless o.class == String
      result.merge(extras(o, d))
    when Array
      result = { "t" => "arr", "v" => o.map { |element| describe(element, d) } }
      result["class"] = Common.class_hex(o.class) unless o.class == Array
      result.merge(extras(o, d))
    when Hash
      result = { "t" => "hash", "v" => o.map { |key, value| [describe(key, d), describe(value, d)] } }
      result["default"] = describe(o.default, d) unless o.default.nil?
      result["identity"] = true if IDENTITY_IN_STREAM && o.respond_to?(:compare_by_identity?) && o.compare_by_identity?
      result["class"] = Common.class_hex(o.class) unless o.class == Hash
      result.merge(extras(o, d))
    when Regexp
      result = { "t" => "regexp", "source" => Common.hex(o.source), "opts" => (o.options & 255),
                 "enc" => Common.encoding_of(o) }
      result["class"] = Common.class_hex(o.class) unless o.class == Regexp
      result.merge(extras(o, d))
    when Class then { "t" => "class", "name" => Common.hex(o.name) }
    when Module then { "t" => "module", "name" => Common.hex(o.name) }
    when Struct
      raise Unknown unless @plain.include?(o.class)
      members = o.members.map { |name| name.to_s }
      result = { "t" => "struct", "class" => Common.class_hex(o.class),
                 "members" => members.each_with_index.map { |name, index| [Common.hex(name), describe(o.to_a[index], d)] } }
      result.merge(extras(o, d))
    else
      core = describe_core(o, d)
      return core if core
      raise Unknown unless @plain.include?(o.class)
      if o.respond_to?(:_dump)
        data = o._dump(-1)
        { "t" => "udump", "class" => Common.class_hex(o.class), "enc" => Common.encoding_of(data),
          "hex" => Common.hex(data) }
      elsif o.respond_to?(:marshal_dump)
        { "t" => "umarshal", "class" => Common.class_hex(o.class), "v" => describe(o.marshal_dump, d) }
      else
        result = { "t" => "obj", "class" => Common.class_hex(o.class),
                   "ivars" => o.instance_variables.map { |name| [Common.hex(name.to_s), describe(o.instance_variable_get(name), d)] } }
        ext = extended_modules(o)
        result["ext"] = ext unless ext.empty?
        result
      end
    end
  end
end

# Core classes the fixtures use, from the standard library of whichever Ruby this is.
%w[set ostruct date bigdecimal].each do |lib|
  begin
    require lib
  rescue LoadError
  end
end
Describe.register(*[defined?(Set) && Set, defined?(OpenStruct) && OpenStruct, Process.times.class].select { |klass| klass })
