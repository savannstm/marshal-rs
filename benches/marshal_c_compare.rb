#!/usr/bin/env ruby
# frozen_string_literal: true

# Companion to benches/marshal_c_compare.rs - times Ruby's stock `Marshal`
# (marshal.c) over the same synthetic fixture, and prints
# "op,records,nanoseconds_per_iter" lines for the Rust driver to parse.

class Record
  def initialize(name, hp, mp, tag)
    @name = name
    @hp = hp
    @mp = mp
    @tag = tag
  end
end

def build_fixture(records)
  (0...records).map { |i| Record.new("Record #{i}", i % 9999, i % 999, :active) }
end

def time_ns(iters)
  3.times { yield }
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  iters.times { yield }
  elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start
  (elapsed / iters * 1_000_000_000).round
end

SIZES = { 100 => 2000, 5000 => 200 }.freeze

SIZES.each do |records, iters|
  fixture = build_fixture(records)
  bytes = Marshal.dump(fixture)

  dump_ns = time_ns(iters) { Marshal.dump(fixture) }
  load_ns = time_ns(iters) { Marshal.load(bytes) }

  puts "dump,#{records},#{dump_ns}"
  puts "load,#{records},#{load_ns}"
end
