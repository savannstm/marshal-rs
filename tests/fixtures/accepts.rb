# Says, for every .bin in a directory, whether Ruby reads the whole file as one Marshal stream. Used for the damaged
# inputs, which the tests compare with what marshal-rs's `load` accepts. Like `load`, bytes after the stream count
# as an error. Prints `ACCEPT <name>` or `REJECT <name>: <error>` per file. The classes the fixtures use are defined
# first, as in verify.rb.
#
#   ruby accepts.rb <classes.rb or classes18.rb> <dir>

require "stringio"
require File.expand_path(ARGV[0])

dir = ARGV[1] or abort("usage: ruby accepts.rb <classes> <dir>")
Dir.entries(dir).select { |entry| entry =~ /\.bin\z/ }.sort.each do |name|
  io = StringIO.new(File.open(File.join(dir, name), "rb") { |file| file.read })
  begin
    Marshal.load(io)
    raise ArgumentError, "trailing data" unless io.eof?
    puts "ACCEPT #{name}"
  rescue Exception => error
    puts "REJECT #{name}: #{error.class}"
  end
end
