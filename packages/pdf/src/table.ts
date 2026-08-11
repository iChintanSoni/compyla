import type { TableCell, TableNode } from "@compyla/core";
import type { PageLayout } from "./layout.js";

const CELL_PADDING = 4;
const HEADER_FILL = "#D9E2F3";
const BORDER_COLOR = "#CCCCCC";
const FONT_SIZE = 10;

function cellText(cell: TableCell): string {
  return cell.runs.map((r) => r.text).join("");
}

function cellHeight(pdf: PDFKit.PDFDocument, cells: readonly TableCell[], colWidth: number, bold: boolean): number {
  pdf.font(bold ? "Helvetica-Bold" : "Helvetica", FONT_SIZE);
  let max = 0;
  for (const cell of cells) {
    const h = pdf.heightOfString(cellText(cell) || " ", { width: colWidth - CELL_PADDING * 2 });
    if (h > max) max = h;
  }
  return max + CELL_PADDING * 2;
}

function drawRow(
  pdf: PDFKit.PDFDocument,
  x0: number,
  y: number,
  colWidth: number,
  cells: readonly TableCell[],
  aligns: readonly ("left" | "center" | "right" | null)[],
  bold: boolean,
  fill: string | undefined,
  height: number,
): void {
  cells.forEach((cell, i) => {
    const x = x0 + i * colWidth;
    pdf.lineWidth(0.5);
    if (fill) {
      pdf.rect(x, y, colWidth, height).fillAndStroke(fill, BORDER_COLOR);
    } else {
      pdf.rect(x, y, colWidth, height).stroke(BORDER_COLOR);
    }
    pdf
      .font(bold ? "Helvetica-Bold" : "Helvetica", FONT_SIZE)
      .fillColor("black")
      .text(cellText(cell), x + CELL_PADDING, y + CELL_PADDING, {
        width: colWidth - CELL_PADDING * 2,
        align: aligns[i] ?? "left",
      });
  });
}

/** Equal-width columns, row height by the tallest wrapped cell, and a new page whenever the next row
 *  wouldn't fit — no `columns` directive for pdf (not in markdown-spec.md §6, matching docx's own table
 *  renderer which doesn't support one either), and no header-row repeat across a page break for M1. */
export function renderTable(pdf: PDFKit.PDFDocument, node: TableNode, layout: PageLayout): void {
  const numCols = node.header.length;
  if (numCols === 0) return;
  const colWidth = layout.contentWidth / numCols;
  const x0 = layout.marginPt;

  const ensureFits = (height: number) => {
    if (pdf.y + height > layout.marginPt + layout.contentHeight) {
      pdf.addPage();
      pdf.y = layout.marginPt;
    }
  };

  const headerHeight = cellHeight(pdf, node.header, colWidth, true);
  ensureFits(headerHeight);
  drawRow(pdf, x0, pdf.y, colWidth, node.header, node.align, true, HEADER_FILL, headerHeight);
  pdf.y += headerHeight;

  for (const row of node.rows) {
    const height = cellHeight(pdf, row, colWidth, false);
    ensureFits(height);
    drawRow(pdf, x0, pdf.y, colWidth, row, node.align, false, undefined, height);
    pdf.y += height;
  }

  pdf.x = layout.marginPt;
}
