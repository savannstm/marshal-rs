# The classes `matrix.rb` dumps, defined before any Ruby loads them. Ruby 1.8 syntax, ASCII names only, so every
# version from 1.8.7 to 4.x defines the same classes.

class MatrixPoint
  def initialize(x, y)
    @x = x
    @y = y
  end
end

class MatrixWrapped
  def initialize(value)
    @value = value
  end

  def marshal_dump
    [@value]
  end

  def marshal_load(array)
    @value = array[0]
  end
end

class MatrixDumped
  def initialize(value)
    @value = value
  end

  def _dump(level)
    @value.to_s
  end

  def self._load(text)
    new(text)
  end
end

module MatrixMixin
end

MatrixPair = Struct.new(:a, :b)
