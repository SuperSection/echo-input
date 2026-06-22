class Echoinput < Formula
  desc "Privacy-first keyboard visualization overlay"
  homepage "https://github.com/SuperSection/echoinput"
  url "https://github.com/SuperSection/echoinput/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_ACTUAL_SHA256"
  license "MIT OR Apache-2.0"

  depends_on "rust" => :build
  depends_on "pkg-config" => :build
  depends_on "cairo"
  depends_on "libxkbcommon"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "EchoInput", shell_output("#{bin}/echoinput --help")
  end
end
