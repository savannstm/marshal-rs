# Has Ruby load what marshal-rs wrote. Runs on Ruby 4 and on Ruby 1.8.7 (with `-Ks` for the latter).
#
#   ruby verify.rb <classes.rb or classes18.rb> <fixtures dir> <produced dir>
#
# For every `<name>.bin` in <produced dir>: it must load in Ruby (every stream in it, for the files holding
# several), and what it loads must be what the original fixture of that name loads to - compared through
# `Describe`, so shared and copied values count as equal. A stream that `Describe` leaves undescribed on both sides
# is only checked to load and is counted as unmodeled; undescribed on one side only is a failure.
# Prints one `FAIL` line per problem, then `verified N files, M failed, U unmodeled streams`, and exits 1 if there
# was any problem. A stream that Ruby cannot load even as its original is printed as `SAME-FAILURE`, which the tests
# accept only where they list it.

require "stringio"
require File.expand_path("ruby_common", File.dirname(__FILE__))
require File.expand_path(ARGV[0])

fixtures_dir = ARGV[1]
produced_dir = ARGV[2]

def load_all(path)
  io = StringIO.new(File.open(path, "rb") { |file| file.read })
  values = []
  values << Marshal.load(io) until io.eof?
  values
end

failures = 0
checked = 0
unmodeled = 0
Dir.entries(produced_dir).select { |entry| entry =~ /\.bin\z/ }.sort.each do |name|
  path = File.join(produced_dir, name)
  checked += 1
  # The original first: a stream Ruby cannot read even as it wrote it is Ruby's failure, not marshal-rs's.
  original = nil
  original_error = nil
  begin
    original = load_all(File.join(fixtures_dir, name))
  rescue Exception => load_error
    original_error = load_error
  end

  begin
    produced = load_all(path)
  rescue Exception => error
    if original_error
      puts "SAME-FAILURE #{name}: Ruby cannot load the original either: #{original_error.class}"
    else
      puts "FAIL #{name}: Ruby cannot load it: #{error.class}: #{error.message}"
      failures += 1
    end
    next
  end

  if original_error
    puts "FAIL #{name}: Ruby cannot load the original: #{original_error.class}: #{original_error.message}"
    failures += 1
    next
  end

  if original.size != produced.size
    puts "FAIL #{name}: #{produced.size} streams, the original has #{original.size}"
    failures += 1
    next
  end

  original.each_with_index do |value, index|
    expected = Describe.desc(value)
    actual = Describe.desc(produced[index])
    if expected.nil? && actual.nil?
      unmodeled += 1
      next
    end
    next if expected == actual
    puts "FAIL #{name}: Ruby loads different data (stream #{index})"
    failures += 1
    break
  end
end

puts "verified #{checked} files, #{failures} failed, #{unmodeled} unmodeled streams"
exit(failures == 0 ? 0 : 1)
