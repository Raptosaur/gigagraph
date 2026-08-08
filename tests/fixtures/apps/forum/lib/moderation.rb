# frozen_string_literal: true

module Moderation
  BANNED = %w[spam scam].freeze

  class Verdict
    attr_reader :reason

    def initialize(allowed, reason = nil)
      @allowed = allowed
      @reason = reason
    end

    def allowed?
      @allowed
    end

    def to_s
      allowed? ? "allowed" : "blocked: #{reason}"
    end

    def self.allow
      new(true)
    end
  end

  def self.review(text)
    hit = BANNED.find { |word| text.downcase.include?(word) }
    hit ? Verdict.new(false, hit) : Verdict.allow
  end

  def self.sanitize(text)
    text.gsub(/<[^>]*>/, "").strip
  end
end

class Feed
  def initialize(threads = [])
    @threads = threads
  end

  def visible
    @threads.select { |t| Moderation.review(t[:body]).allowed? }
  end

  def add(thread)
    @threads << thread
    self
  end

  private

  def normalise(thread)
    thread.transform_keys(&:to_sym)
  end
end
