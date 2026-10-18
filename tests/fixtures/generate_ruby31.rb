# coding: utf-8
# The times whose zone is given with `in:` (Ruby 3.1 and later), and what uses them. Loaded by generate.rb on Ruby 3.1
# and later.

TIMES.merge!(
  "offset" => Time.at(0, 123_456_789, :nsec, in: "+09:00"),
  "rational" => Time.at(Rational(1, 3), in: "UTC"),
  "epoch" => Time.at(0, in: "UTC"),
  "before_epoch" => Time.at(-1, in: "UTC")
)
%w[offset rational epoch before_epoch].each { |name| emit("core_time_#{name}", "core", TIMES[name]) }
emit("core_time_shared", "core", [TIMES["utc"], TIMES["utc"], TIMES["offset"], TIMES["utc"]])
emit("core_time_in_hash", "core", { TIMES["utc"] => TIMES["offset"], :list => TIMES.values })
emit("core_range_time", "core", TIMES["epoch"]..TIMES["utc"])
