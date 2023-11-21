#!/usr/bin/env python3

import csv
import argparse
import sys


def main(args):
    rdr = csv.reader(args.infile)
    wtr = csv.writer(sys.stdout)
    # first row is the header
    wtr.writerow(next(rdr))
    for row in rdr:
        if len(row) >= args.column:
            try:
                toconv = row[args.column]
                convd = toconv[0] + toconv[-6:]
                wtr.writerow(row[:(args.column)]+[convd]+row[(args.column+1):])
            except IndexError as e:
                print(row, file=sys.stderr)
                raise e


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Alter a CSV file to rewrite one of the columns \
        from ABS 11-digit SA1 maincode format to 7-digit shortcode format.\n\
         Writes to standard output.")
    parser.add_argument("--column", type=int, metavar="INT",
                        help="Column to convert from SA1 maincode to SA1 7-digit format (0-indexed)")
    parser.add_argument('infile', nargs='?', type=argparse.FileType('r'),
                        default=sys.stdin)

    args = parser.parse_args()

    main(args)
