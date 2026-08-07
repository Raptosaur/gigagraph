module Format
  def self.currency(cents)
    "$#{cents / 100.0}"
  end
end
