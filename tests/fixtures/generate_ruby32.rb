# coding: utf-8
# The fixtures that use the `Data` classes of classes_ruby32.rb. Loaded by generate.rb on Ruby 3.2 and later.

emit("obj_data_define", "object", Coördinate.new(x: 1, y: "２"))
emit("core_data", "core", [Money.new(amount: 5, currency: "円"), Money.new(1.5, :usd), EmptyData.new, Coördinate.new(x: [3], y: { a: 1 })])
