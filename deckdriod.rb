class Kdev < Formula
  desc "Professional Android Development Dashboard"
  homepage "https://github.com/theasmat/kdev"
  version "0.1.0" # Update this with your actual version

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/theasmat/kdev/releases/download/v#{version}/kdev-aarch64-apple-darwin.tar.gz"
      # sha256 "REPLACE_WITH_ACTUAL_SHA256"
    else
      url "https://github.com/theasmat/kdev/releases/download/v#{version}/kdev-x86_64-apple-darwin.tar.gz"
      # sha256 "REPLACE_WITH_ACTUAL_SHA256"
    end
  elsif OS.linux?
    url "https://github.com/theasmat/kdev/releases/download/v#{version}/kdev-x86_64-unknown-linux-musl.tar.gz"
    # sha256 "REPLACE_WITH_ACTUAL_SHA256"
  end

  def install
    bin.install "kdev"
  end

  test do
    system "#{bin}/kdev", "--help"
  end
end
