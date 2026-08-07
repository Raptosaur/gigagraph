require "json"
require_relative "helper"

# Turns raw report entries into printable summaries.
class ReportService
  def initialize(entries, currency: "USD")
    @entries = entries
    @currency = currency
  end

  def self.from_json(payload)
    new(JSON.parse(payload))
  end

  def summarize(limit = 10)
    rows = @entries.take(limit)
    rows.each do |row|
      puts render_row(row)
    end
    total_for(rows)
  end

  def render_row(row)
    label = self.build_label(row)
    row.empty? ? "(blank)" : label
  end

  def build_label(row)
    Format.label(row.fetch(:name))
  end

  def total_for(rows)
    total = 0
    i = 0
    while i < rows.length
      total += rows[i].fetch(:amount, 0)
      i += 1
    end
    total
  end
end

service = ReportService.from_json(ARGV.first)
service.summarize(5)
