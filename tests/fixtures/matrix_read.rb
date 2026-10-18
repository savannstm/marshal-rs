# Loads every stream of every .bin in a directory with the Ruby running it, after defining the classes `matrix.rb`
# dumps. Per stream it prints one line: `DESC <file>#<n> <json>` with what `Describe` (ruby_common.rb) says the value
# holds, `UNMODELED <file>#<n>` when `Describe` does not model it, or `FAIL <file>#<n>: <error>` when the file does
# not load (the streams of that file are not described). Ends with `loaded N failed M`.
# Ruby 1.8 syntax, like `matrix.rb`.
#
#   ruby matrix_read.rb <dir>

require "stringio"
require File.join(File.dirname(__FILE__), "ruby_common")
require File.join(File.dirname(__FILE__), "matrix_classes")

# The matrix's own classes, and Object for the extended value (a plain object, described by its ivars and extends).
Describe.register(Object, MatrixPoint, MatrixWrapped, MatrixDumped, MatrixPair)

# The same description with the entries of every hash in sorted order. Ruby hashes compare equal in any order, and
# Ruby 1.8 iterates them by the ids of their symbol keys, which differ from one process to the next.
def canonical(desc)
  case desc
  when Hash
    result = {}
    desc.each { |key, value| result[key] = canonical(value) }
    result["v"] = result["v"].sort_by { |pair| MiniJSON.dump(pair) } if result["t"] == "hash"
    result
  when Array
    desc.map { |element| canonical(element) }
  else
    desc
  end
end

dir = ARGV[0] or abort("usage: ruby matrix_read.rb <dir>")
loaded = 0
failed = 0
Dir.entries(dir).select { |entry| entry =~ /\.bin\z/ }.sort.each do |name|
  values = []
  begin
    io = StringIO.new(File.open(File.join(dir, name), "rb") { |file| file.read })
    until io.eof?
      values << Marshal.load(io)
    end
  rescue Exception => error
    puts "FAIL #{name}##{values.size}: #{error.class}: #{error.message}"
    failed += 1
    next
  end
  # Outside the rescue above: a bug in Describe must not look like a file that fails to load.
  values.each_with_index do |value, index|
    desc = Describe.desc(value)
    if desc.nil?
      puts "UNMODELED #{name}##{index}"
    else
      puts "DESC #{name}##{index} #{MiniJSON.dump(canonical(desc))}"
    end
  end
  loaded += 1
end
puts "loaded #{loaded} failed #{failed}"
