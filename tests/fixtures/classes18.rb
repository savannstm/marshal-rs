# The classes the Ruby 1.8.7 fixtures use. ASCII only on purpose: Ruby 1.8 has no source encodings, so the
# Shift_JIS names are spelled as bytes and the interpreter runs with `-Ks`, which makes it accept them in constant,
# instance variable and symbol names the way RPG Maker XP's scripts did.
#
# Loaded by the generator (to dump instances) and by the verifier (to load the streams marshal-rs wrote), which is
# why anything with `_dump` / `marshal_dump` keeps what it was given.

require File.expand_path("ruby_common", File.dirname(__FILE__))

module Sjis
  def self.hex(text)
    [text].pack("H*")
  end

  ACTOR = hex("8341834e835e815b")
  NAME = hex("96bc914f")
  MEMO = hex("83818382")
  HERO = hex("8371815b838d815b")
  PARAMETER = hex("837083898381815b835e")
  STRUCT = hex("8d5c91a291cc")
  MEMBER = hex("83818393836f815b")
  STRING = hex("95b68e9a97f1")
  EXTENSION = hex("8a6792a3")
  TEXT = hex("8365834c83588367")
  JAPANESE = hex("93fa967b8cea")
  WARRIOR = hex("90ed8e6d")
  TAG = hex("835e834f")
  KIND = hex("8eed97de")
  COEFFICIENT = hex("8c579094")
  MAP = hex("837d83628376")
  KEY = hex("834c815b")
  DUMP = hex("835f83938376")
  SUB = hex("83548375")
  CONTENT = hex("92869067")
  VALUE = hex("926c")

  def self.define(name, klass)
    Object.const_set(name, klass)
    klass
  end
end

# `Actor<アクター>` and so on: a constant has to start with an ASCII capital.
$actor_class = Sjis.define("Actor" + Sjis::ACTOR, Class.new do
  def initialize(id)
    instance_variable_set("@" + Sjis::NAME, Sjis::HERO + id.to_s)
    instance_variable_set("@id", id)
    instance_variable_set("@" + Sjis::MEMO, [Sjis::TEXT, :plain, Sjis::KEY.to_sym])
    instance_variable_set("@hp", 100.0 / (id + 3))
    instance_variable_set("@ascii", "ascii text")
  end
end)

$plain_class = Sjis.define("Plain", Class.new do
  def initialize
    @a = 1
    @b = "two"
    @c = [3.5, :four, nil]
  end
end)

$string_sub = Sjis.define("Str" + Sjis::STRING, Class.new(String))
$array_sub = Sjis.define("Arr" + Sjis::STRING, Class.new(Array))
$hash_sub = Sjis.define("Hsh" + Sjis::STRING, Class.new(Hash))

$extension_module = Module.new
Sjis.define("Ext" + Sjis::EXTENSION, $extension_module)
$extra_module = Module.new
Sjis.define("Extra", $extra_module)

$struct_class = Sjis.define("Pt" + Sjis::STRUCT, Struct.new(Sjis::MEMBER.to_sym, :plain, Sjis::KEY.to_sym))

# `_dump` returning bytes in a legacy encoding
$dump_class = Sjis.define("Dump" + Sjis::DUMP, Class.new do
  def initialize(data)
    @data = data
  end

  def _dump(level)
    @data
  end

  def self._load(data)
    new(data)
  end
end)

# `marshal_dump`
$marshal_class = Sjis.define("Marsh" + Sjis::DUMP, Class.new do
  def initialize(parts)
    @parts = parts
  end

  def marshal_dump
    @parts
  end

  def marshal_load(parts)
    @parts = parts
  end
end)

module RPG
  class Table
    def initialize(values)
      @raw = ([2, values.size, 1, values.size, 1] + values).pack("V5v*")
    end

    def _dump(level)
      @raw
    end

    def self._load(raw)
      table = allocate
      table.instance_variable_set("@raw", raw)
      table
    end
  end

  class Color
    def initialize(*channels)
      @raw = channels.pack("E4")
    end

    def _dump(level)
      @raw
    end

    def self._load(raw)
      color = allocate
      color.instance_variable_set("@raw", raw)
      color
    end
  end

  class Actor
    def initialize(id)
      @id = id
      @name = Sjis::HERO + id.to_s
      @class_id = id % 7
      @exp_basis = 30 + id * 0.5
      @parameters = Table.new((1..12).map { |i| i * id })
      @color = Color.new(0.1 * id, 1 / 3.0, 255.0, 0.0)
      @note = Sjis::MEMO + " " + id.to_s
      @tags = [Sjis::TAG.to_sym, :tag, "tag" + id.to_s, nil]
      @weapon = { :weapon_id => id, Sjis::NAME => Sjis::KIND, Sjis::KIND.to_sym => [1, 2, 3] }
    end
  end

  class Map
    def initialize
      @tileset_id = 1
      @width = 20
      @height = 15
      @events = { 1 => Actor.new(1), 2 => Actor.new(2) }
      @data = Table.new((0...300).map { |i| i % 17 })
    end
  end
end

Describe.register($actor_class, $plain_class, $struct_class, $dump_class, $marshal_class, RPG::Table, RPG::Color, RPG::Actor,
                  RPG::Map)
