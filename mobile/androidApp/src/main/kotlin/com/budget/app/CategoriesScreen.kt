package com.budget.app

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.budget.shared.api.BudgetMode
import com.budget.shared.viewmodel.CategoriesViewModel
import com.budget.shared.viewmodel.CategoryDisplayItem
import com.budget.shared.viewmodel.CategorySection
import java.text.NumberFormat
import java.util.Currency
import java.util.Locale
import kotlin.math.abs

// -- Kanagawa palette colors for budget modes --------------------------------

private val CrystalBlue = Color(0xFF7E9CD8)
private val OniViolet = Color(0xFF957FB8)
private val AutumnYellow = Color(0xFFDCA561)

private fun modeColor(mode: BudgetMode?): Color =
    when (mode) {
      BudgetMode.MONTHLY -> CrystalBlue
      BudgetMode.ANNUAL -> OniViolet
      BudgetMode.PROJECT -> AutumnYellow
      null -> Color.Unspecified
    }

// -- Currency formatting -----------------------------------------------------

private val EuroCurrencyFormat: NumberFormat =
    NumberFormat.getCurrencyInstance(Locale.GERMANY).apply {
      currency = Currency.getInstance("EUR")
    }

private fun formatBudgetAmount(value: String?): String {
  if (value == null) return "No budget"
  val d = value.toDoubleOrNull() ?: return value
  return synchronized(EuroCurrencyFormat) {
    EuroCurrencyFormat.maximumFractionDigits = 0
    EuroCurrencyFormat.format(abs(d))
  }
}

// -- Root screen -------------------------------------------------------------

@Composable
fun CategoriesScreen(
    viewModel: CategoriesViewModel,
    onAddCategory: () -> Unit = {},
    onEditCategory: (String) -> Unit = {},
    modifier: Modifier = Modifier,
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  Box(modifier = modifier.fillMaxSize()) {
    when {
      state.loading && state.sections.isEmpty() -> {
        CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
      }
      state.error != null && state.sections.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = state.error ?: "Unknown error",
              color = MaterialTheme.colorScheme.error,
          )
          Spacer(modifier = Modifier.height(12.dp))
          TextButton(onClick = viewModel::refresh) { Text("Retry") }
        }
      }
      state.sections.isEmpty() -> {
        Text(
            text = "No categories found.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
        )
      }
      else -> {
        CategoriesContent(
            sections = state.sections,
            expandedSections = state.expandedSections,
            onToggleSection = viewModel::toggleSection,
            onCategoryClick = onEditCategory,
        )
      }
    }
    FloatingActionButton(
        onClick = onAddCategory,
        modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
    ) {
      Icon(Icons.Default.Add, contentDescription = "Add category")
    }
  }
}

// -- Content -----------------------------------------------------------------

@Composable
private fun CategoriesContent(
    sections: List<CategorySection>,
    expandedSections: Set<BudgetMode?>,
    onToggleSection: (BudgetMode?) -> Unit,
    onCategoryClick: (String) -> Unit,
) {
  LazyColumn(
      modifier = Modifier.fillMaxSize(),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 80.dp),
      verticalArrangement = Arrangement.spacedBy(12.dp),
  ) {
    for (section in sections) {
      val expanded = section.mode in expandedSections
      item(key = "section-${section.mode}") {
        SectionCard(
            section = section,
            expanded = expanded,
            onToggle = { onToggleSection(section.mode) },
            onCategoryClick = onCategoryClick,
        )
      }
    }
  }
}

// -- Section card with header + collapsible items ----------------------------

@Composable
private fun SectionCard(
    section: CategorySection,
    expanded: Boolean,
    onToggle: () -> Unit,
    onCategoryClick: (String) -> Unit,
) {
  val color = modeColor(section.mode)

  ElevatedCard(modifier = Modifier.fillMaxWidth().animateContentSize()) {
    Column {
      // Section header
      Row(
          modifier = Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(16.dp),
          verticalAlignment = Alignment.CenterVertically,
      ) {
        if (section.mode != null) {
          Box(
              modifier = Modifier.size(10.dp).clip(CircleShape).background(color),
          )
          Spacer(modifier = Modifier.width(8.dp))
        }
        Text(
            text = section.label,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.weight(1f),
        )
        Text(
            text = "${section.categories.size}",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier =
                Modifier.clip(RoundedCornerShape(4.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .padding(horizontal = 6.dp, vertical = 2.dp),
        )
        Spacer(modifier = Modifier.width(4.dp))
        Icon(
            imageVector = if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
            contentDescription = if (expanded) "Collapse" else "Expand",
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }

      // Category items
      if (expanded) {
        for (item in section.categories) {
          CategoryItem(item = item, onClick = { onCategoryClick(item.id) })
        }
        Spacer(modifier = Modifier.height(8.dp))
      }
    }
  }
}

// -- Individual category item ------------------------------------------------

@Composable
private fun CategoryItem(item: CategoryDisplayItem, onClick: () -> Unit) {
  val startPadding = if (item.isChild) 40.dp else 16.dp

  Row(
      modifier =
          Modifier.fillMaxWidth()
              .clickable(onClick = onClick)
              .padding(start = startPadding, end = 16.dp, top = 6.dp, bottom = 6.dp),
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Column(modifier = Modifier.weight(1f)) {
      Row(verticalAlignment = Alignment.CenterVertically) {
        if (item.isChild && item.parentName != null) {
          Text(
              text = "${item.parentName} > ",
              style = MaterialTheme.typography.bodyMedium,
              color = MaterialTheme.colorScheme.onSurfaceVariant,
              maxLines = 1,
          )
        }
        Text(
            text = item.name,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
      }
      Text(
          text =
              if (item.transactionCount == 1) "1 transaction"
              else "${item.transactionCount} transactions",
          style = MaterialTheme.typography.bodySmall,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
    }

    Text(
        text = formatBudgetAmount(item.budgetAmount),
        style = MaterialTheme.typography.bodyMedium,
        color =
            if (item.budgetAmount != null) MaterialTheme.colorScheme.onSurface
            else MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}
