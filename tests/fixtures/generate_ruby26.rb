# coding: utf-8
# The fixtures that use the classes of classes_ruby26.rb: non-ASCII class names, a constant made with `const_set`, and
# a `keyword_init` struct. Loaded by generate.rb on Ruby 2.6 and later.

emit("obj_user_class_string", "object", Ｓ文字列.new("サブクラス"))
emit("obj_user_class_array", "object", Ａ配列.new([1, "é", :"日本"]))
emit("obj_user_class_hash", "object", Ｈハッシュ.new.merge("キー" => 1))
emit("obj_user_class_regexp", "object", Ｒ正規表現.new("a+"))
emit("obj_nested_module_class", "object", Ｎ日本::Ｃクラス.new)
emit("obj_struct_class", "object", Ünit.new("ñ", "ü"))
emit("obj_class_and_module_refs", "object", [Café, Ｎ日本, Ｎ日本::Ｃクラス, Módulo, String, Comparable])
emit("core_struct_keyword", "core", KeywordStruct.new(a: 1, b: "x"))

# Wrappers stacked on one value: a subclass, `extend`, instance variables and an encoding.
combo = Ｓ文字列.new("サブ".encode("Shift_JIS")).tap { |s| s.instance_variable_set(:@i, 1); s.extend(Módulo) }
emit("combo_string", "core", combo)
emit("combo_array", "core", Ａ配列.new([1, 2]).tap { |a| a.instance_variable_set(:@i, 1); a.extend(Módulo).extend(Extra) })
emit("combo_hash", "core", Ｈハッシュ.new.merge(1 => 2).tap { |h| h.default = 3; h.instance_variable_set(:@i, 1); h.extend(Módulo) })
emit("combo_regexp", "core", Ｒ正規表現.new("a+").tap { |r| r.instance_variable_set(:@i, 1) })

change("user_class_string", "x") { |s| Ｓ文字列.new(s) }
change("user_class_array", [1, "é"]) { |a| Ａ配列.new(a) }
change("user_class_hash", { 1 => 2 }) { |h| Ｈハッシュ.new.merge(h) }
change("user_class_regexp", /a+/i) { |r| Ｒ正規表現.new("a+", Regexp::IGNORECASE) }
change("user_class_cleared", Ｓ文字列.new("x")) { |s| String.new(s) }
change("user_class_cleared_array", Ａ配列.new([1])) { |a| Array.new(a) }
