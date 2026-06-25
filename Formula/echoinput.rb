class Echoinput < Formula
  desc "Privacy-first keyboard visualization overlay"
  homepage "https://github.com/SuperSection/echoinput"
  url "https://github.com/SuperSection/echoinput/archive/refs/tags/v0.1.0-alpha.1.tar.gz"
  sha256 "aec128360539fb34bf78ca8e04b12a34929a27f0745f40012ca079177e351d45"
  license "MIT OR Apache-2.0"

  depends_on "rust" => :build
  depends_on "pkg-config" => :build
  depends_on "cairo"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "EchoInput", shell_output("#{bin}/echoinput --help")
  end
end
