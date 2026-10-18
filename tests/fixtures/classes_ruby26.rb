# coding: utf-8
# The classes Ruby 2.6 and later can name: class names that start with a non-ASCII capital (Ruby before 2.6 only
# takes an ASCII capital to start a constant), a constant made with `const_set` from one, and `keyword_init` structs
# (2.5 and later). Loaded by classes.rb, and only on those Rubies.

require_relative "ruby_common"

class Ｓ文字列 < String; end
class Ａ配列 < Array; end
class Ｒ正規表現 < Regexp; end
class Ｈハッシュ < Hash; end
module Ｎ日本; class Ｃクラス; def initialize; (@値 = 1); end; end; end
Object.const_set(:Ünit, Struct.new(:ñame, :value))
KeywordStruct = Struct.new(:a, :b, keyword_init: true)

Describe.register(KeywordStruct, Ｓ文字列, Ａ配列, Ｈハッシュ, Ｎ日本::Ｃクラス, Ünit)
