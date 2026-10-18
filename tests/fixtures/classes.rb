# coding: utf-8
# The classes the Ruby 4 fixtures use. Loaded by the generator (to dump instances) and by the verifier (to load the
# streams marshal-rs wrote), which is why anything with `_dump` / `marshal_dump` keeps what it was given instead
# of throwing it away: loading and describing the result must show the same data.

require_relative "ruby_common"

class Café
  def initialize
    @é = 1
    @日本語 = "UTF-8"
    @sjis = "シフトJIS".encode("Shift_JIS")
    @bytes = "\xff\x00".b
    @sym = :"記号"
    @nested = [self.class.name, nil, true, false]
  end
end

class PlainString < String; end
module Módulo; end

Point = Struct.new(:名前, :値)

class Dumpy # `_dump` returning text in a legacy encoding: `u` with an encoding ivar
  def initialize(data = "ダンプ".encode("Shift_JIS")); (@data = data); end
  def _dump(_level); @data; end
  def self._load(data); new(data); end
end

class DumpyBinary # `_dump` returning bytes
  def initialize(data = [1, 2, 3, 0xff, 0xfe].pack("C*")); (@data = data); end
  def _dump(_level); @data; end
  def self._load(data); new(data); end
end

class Marshaled # `marshal_dump`: `U`
  def initialize(parts = ["マーシャル".encode("Shift_JIS"), "é", :"記号", 1.5]); (@parts = parts); end
  def marshal_dump; @parts; end
  def marshal_load(parts); (@parts = parts); end
end

class ScalarMarshaled # `marshal_dump` returning a scalar
  def initialize(value = 42); (@value = value); end
  def marshal_dump; @value; end
  def marshal_load(value); (@value = value); end
end

class MarshaledArray # `marshal_dump` returning an array that has an ivar of its own
  def initialize(array = [1, 2].tap { |list| list.instance_variable_set(:@label, "ラベル") }); (@array = array); end
  def marshal_dump; @array; end
  def marshal_load(array); (@array = array); end
end

class Plain
  def initialize; (@a = 1; @b = "two"; @c = [3.5, :four, nil]); end
end

module Extra; end
module Extra2; end

module RPG
  class Table
    def initialize(values); (@raw = [2, values.size, 1, values.size, 1, *values].pack("l<5s<*")); end
    def _dump(_level); @raw; end
    def self._load(raw); allocate.tap { |table| table.instance_variable_set(:@raw, raw) }; end
  end

  class Color
    def initialize(*channels); (@raw = channels.pack("E4")); end
    def _dump(_level); @raw; end
    def self._load(raw); allocate.tap { |color| color.instance_variable_set(:@raw, raw) }; end
  end

  class Actor
    def initialize(id)
      @id = id
      @name = "アクター#{id}".encode("Shift_JIS")
      @class_id = id % 7
      @battler_name = "バトラー#{id}".encode("Windows-31J")
      @exp_basis = 30 + id * 0.5
      @parameters = Table.new((1..12).map { |i| i * id })
      @color = Color.new(0.1 * id, 1 / 3.0, 255.0, 0.0)
      @note = id.even? ? "メモ #{id}" : "メモ #{id}".encode("EUC-JP")
      @tags = [:"タグ", :tag, "tag#{id}", nil]
      @weapon = { :weapon_id => id, "名前" => "剣", :"属性" => [1, 2, 3] }
    end
  end

  class Event
    def initialize(name); (@name = name; @pages = [{ :list => ["テキスト".encode("Shift_JIS"), 101], :id => 1 }]); end
  end

  class Map
    def initialize
      @tileset_id = 1
      @width = 20
      @height = 15
      @events = { 1 => Event.new("イベント001".encode("Shift_JIS")), 2 => Event.new("EV002") }
      @data = Table.new(Array.new(300) { |i| i % 17 })
    end
  end
end

class MyError < StandardError # an exception with state of its own
  def initialize(message = "カスタム", code = 7); (super(message); @code = code; @label = "ラベル"); end
end

class Mut # for the tests that change a value and compare with what Ruby writes for the changed one
  def initialize; (@a = 1; @b = "two"; @c = :three; @d = [4]; @e = nil; @f = 5.5); end
end

MutStruct = Struct.new(:a, :b, :c, :d, :e)
Describe.register(MyError, Mut, MutStruct, Café, Point, Dumpy, DumpyBinary, Marshaled,
                  ScalarMarshaled, MarshaledArray, Plain, RPG::Table, RPG::Color, RPG::Actor, RPG::Event, RPG::Map)

# Classes that older Rubies cannot name or define (non-ASCII class names, `Data`, `keyword_init`) live in their own
# files, which only the Rubies that accept them load.
require_relative "classes_ruby26" if RUBY_VERSION >= "2.6"
require_relative "classes_ruby32" if RUBY_VERSION >= "3.2"

