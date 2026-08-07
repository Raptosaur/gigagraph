# String helpers shared by the reporting scripts.
module Format
  def self.label(value)
    text = value.to_s.strip
    text.empty? ? "unknown" : text.capitalize
  end

  def self.pad(value, width = 8)
    label(value).ljust(width)
  end

  def self.divider
    "-" * 12
  end
end
