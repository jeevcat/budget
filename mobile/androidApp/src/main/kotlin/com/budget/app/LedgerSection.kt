package com.budget.app

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.CashFlowItem
import com.budget.shared.api.LedgerSummary
import com.budget.shared.api.PaceIndicator
import kotlin.math.abs

// Pace colors matching DashboardScreen
private val PendingColor = Color(0xFF938AA9)
private val UnderBudgetColor = Color(0xFF76946A)
private val OnTrackColor = Color(0xFF7E9CD8)
private val AbovePaceColor = Color(0xFFDCA561)
private val OverBudgetColor = Color(0xFFC34043)

private fun paceColor(pace: PaceIndicator): Color =
    when (pace) {
      PaceIndicator.PENDING -> PendingColor
      PaceIndicator.UNDER_BUDGET -> UnderBudgetColor
      PaceIndicator.ON_TRACK -> OnTrackColor
      PaceIndicator.ABOVE_PACE -> AbovePaceColor
      PaceIndicator.OVER_BUDGET -> OverBudgetColor
    }

// -- Ledger content for LazyColumn -----------------------------------------

fun LazyListScope.ledgerContent(
    ledger: LedgerSummary,
    statuses: List<BudgetStatus>,
    selectedCategoryId: String?,
    onCategoryClick: (String) -> Unit,
    onCashFlowItemClick: (CashFlowItem) -> Unit,
) {
  // IN section
  if (ledger.income.isNotEmpty()) {
    item(key = "ledger-in-label") { LedgerSectionLabel(text = "IN") }
    items(ledger.income, key = { "ledger-in-${it.categoryId ?: it.label}" }) { item ->
      LedgerIncomeRow(item = item, onClick = { onCashFlowItemClick(item) })
    }
    item(key = "ledger-in-subtotal") {
      LedgerSubtotalRow(
          label = "Total In",
          amount = formatAmount(ledger.totalIn, showSign = true),
          amountColor = UnderBudgetColor,
      )
    }
  }

  // OUT section
  item(key = "ledger-out-label") { LedgerSectionLabel(text = "OUT") }
  items(statuses, key = { "ledger-out-${it.categoryId}" }) { status ->
    LedgerBudgetRow(
        status = status,
        barMax = ledger.barMax,
        selected = status.categoryId == selectedCategoryId,
        onClick = { onCategoryClick(status.categoryId) },
    )
  }
  if (ledger.unbudgeted.isNotEmpty()) {
    item(key = "ledger-unbudgeted-divider") {
      HorizontalDivider(
          modifier = Modifier.padding(vertical = 4.dp),
          color = MaterialTheme.colorScheme.outlineVariant,
      )
    }
    items(ledger.unbudgeted, key = { "ledger-ub-${it.categoryId ?: it.label}" }) { item ->
      LedgerUnbudgetedRow(item = item, onClick = { onCashFlowItemClick(item) })
    }
  }
  item(key = "ledger-out-subtotal") {
    LedgerSubtotalRow(
        label = "Total Out",
        amount = formatAmount(ledger.totalOut),
        amountColor = OverBudgetColor,
    )
  }

  // NET section
  item(key = "ledger-net") {
    LedgerNetRow(
        net = ledger.net,
        saved = ledger.saved,
        hasIncome = ledger.income.isNotEmpty(),
    )
  }
}

// -- Section label ---------------------------------------------------------

@Composable
private fun LedgerSectionLabel(text: String) {
  Text(
      text = text,
      style = MaterialTheme.typography.labelMedium,
      color = MaterialTheme.colorScheme.onSurfaceVariant,
      modifier = Modifier.padding(top = 8.dp, bottom = 2.dp),
  )
}

// -- Income row ------------------------------------------------------------

@Composable
private fun LedgerIncomeRow(item: CashFlowItem, onClick: () -> Unit) {
  Row(
      modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = item.label,
        style = MaterialTheme.typography.bodyMedium,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )
    Text(
        text = formatAmount(item.amount, showSign = true),
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
        color = UnderBudgetColor,
    )
  }
}

// -- Budget row (with pace dot + inline bar) -------------------------------

@Composable
private fun LedgerBudgetRow(
    status: BudgetStatus,
    barMax: Double,
    selected: Boolean,
    onClick: () -> Unit,
) {
  val pace = status.pace
  val color = paceColor(pace)
  val isOverBudget = pace == PaceIndicator.OVER_BUDGET
  val bgModifier =
      if (isOverBudget) {
        Modifier.background(OverBudgetColor.copy(alpha = 0.08f))
      } else {
        Modifier
      }
  val containerModifier =
      if (selected) {
        Modifier.background(MaterialTheme.colorScheme.secondaryContainer)
      } else {
        bgModifier
      }

  Row(
      modifier =
          containerModifier
              .fillMaxWidth()
              .clickable(onClick = onClick)
              .padding(vertical = 6.dp, horizontal = 4.dp),
      verticalAlignment = Alignment.CenterVertically,
      horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    // Pace dot
    Canvas(modifier = Modifier.size(8.dp).clip(CircleShape)) { drawCircle(color = color) }

    // Category name
    Text(
        text = status.categoryName,
        style = MaterialTheme.typography.bodyMedium,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )

    // Inline spend bar
    InlineSpendBar(
        spent = status.spent,
        budget = status.budgetAmount,
        barMax = barMax,
        pace = pace,
    )

    // Budget amount
    Text(
        text = formatAmount(status.budgetAmount),
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )

    // Spent amount
    Text(
        text = formatAmount(status.spent),
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
    )

    // Delta (remaining)
    Text(
        text = formatAmount(status.remaining, showSign = true),
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        color = color,
    )
  }
}

// -- Unbudgeted row --------------------------------------------------------

@Composable
private fun LedgerUnbudgetedRow(item: CashFlowItem, onClick: () -> Unit) {
  Row(
      modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = item.label,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )
    Text(
        text = formatAmount(item.amount),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}

// -- Subtotal row ----------------------------------------------------------

@Composable
private fun LedgerSubtotalRow(label: String, amount: String, amountColor: Color) {
  HorizontalDivider(
      modifier = Modifier.padding(top = 6.dp),
      color = MaterialTheme.colorScheme.outlineVariant,
  )
  Row(
      modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = label,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Bold,
    )
    Text(
        text = amount,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Bold,
        color = amountColor,
    )
  }
}

// -- Net row ---------------------------------------------------------------

@Composable
private fun LedgerNetRow(net: Double, saved: Double, hasIncome: Boolean) {
  HorizontalDivider(
      modifier = Modifier.padding(top = 4.dp),
      thickness = 2.dp,
      color = MaterialTheme.colorScheme.outline,
  )
  Column(modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Text(
          text = "Net",
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Bold,
      )
      Text(
          text = formatAmount(net, showSign = true),
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Bold,
          color = if (net < 0) OverBudgetColor else UnderBudgetColor,
      )
    }
    if (hasIncome) {
      Spacer(modifier = Modifier.height(2.dp))
      Row(
          modifier = Modifier.fillMaxWidth(),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = "Saved",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = formatAmount(saved, showSign = true),
            style = MaterialTheme.typography.bodySmall,
            fontWeight = FontWeight.Medium,
            color = if (saved < 0) OverBudgetColor else UnderBudgetColor,
        )
      }
    }
  }
}

// -- Inline spend bar ------------------------------------------------------

@Composable
private fun InlineSpendBar(
    spent: Double,
    budget: Double,
    barMax: Double,
    pace: PaceIndicator,
) {
  val fillColor = paceColor(pace)
  val trackColor = MaterialTheme.colorScheme.surfaceVariant
  val markColor = MaterialTheme.colorScheme.outline

  Canvas(modifier = Modifier.widthIn(min = 48.dp, max = 80.dp).height(8.dp)) {
    val w = size.width
    val h = size.height
    val radius = CornerRadius(h / 2, h / 2)

    // Track background
    drawRoundRect(
        color = trackColor,
        size = Size(w, h),
        cornerRadius = radius,
    )

    // Fill bar
    val fillFraction = if (barMax > 0) (abs(spent) / barMax).coerceIn(0.0, 1.0) else 0.0
    if (fillFraction > 0) {
      drawRoundRect(
          color = fillColor.copy(alpha = 0.8f),
          size = Size((w * fillFraction).toFloat(), h),
          cornerRadius = radius,
      )
    }

    // Budget mark (vertical line)
    if (budget > 0 && barMax > 0) {
      val markX = (budget / barMax * w).toFloat().coerceIn(0f, w)
      drawLine(
          color = markColor,
          start = Offset(markX, 0f),
          end = Offset(markX, h),
          strokeWidth = 2f,
          cap = StrokeCap.Round,
      )
    }
  }
}
