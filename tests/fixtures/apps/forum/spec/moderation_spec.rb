require "moderation"

RSpec.describe Moderation do
  describe ".review" do
    it "blocks banned words" do
      expect(Moderation.review("this is spam").allowed?).to be false
    end

    it "allows clean text" do
      expect(Moderation.review("hello there").allowed?).to be true
    end

    context "with mixed case" do
      it "matches case-insensitively" do
        expect(Moderation.review("SPAM").allowed?).to be false
      end
    end
  end

  describe ".sanitize" do
    it "strips tags" do
      expect(Moderation.sanitize("<b>hi</b>")).to eq "hi"
    end
  end

  def build_thread(body)
    { body: body }
  end
end
