# coding: utf-8
# The `Data` classes (Ruby 3.2 and later). Loaded by classes.rb, and only on those Rubies.

require_relative "ruby_common"

Coördinate = Data.define(:x, :y)
Money = Data.define(:amount, :currency)
EmptyData = Data.define

Describe.register(Coördinate, Money, EmptyData)
