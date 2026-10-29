"""Command line entry point: itb-analysis <command>."""
import sys


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    print("itb-analysis", " ".join(argv))
    return 0


if __name__ == "__main__":
    sys.exit(main())
