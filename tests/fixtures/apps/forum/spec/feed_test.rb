require "minitest/autorun"
require "moderation"

class FeedTest < Minitest::Test
  def setup
    @feed = Feed.new
  end

  def test_visible_hides_blocked_threads
    @feed.add(body: "spam")
    assert_empty @feed.visible
  end

  def test_add_returns_self_for_chaining
    assert_equal @feed, @feed.add(body: "hi")
  end

  def teardown
    @feed = nil
  end
end
