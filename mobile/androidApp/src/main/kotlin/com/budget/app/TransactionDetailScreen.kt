package com.budget.app

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.budget.shared.api.CategoryMethod
import com.budget.shared.api.Transaction
import com.budget.shared.viewmodel.DisplayCategory
import com.budget.shared.viewmodel.TransactionsUiState
import com.budget.shared.viewmodel.TransactionsViewModel
import java.text.NumberFormat
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.Currency
import java.util.Locale
import kotlin.math.abs

// -- Colors -----------------------------------------------------------------

private val ExpenseColor = Color(0xFFC34043)
private val IncomeColor = Color(0xFF76946A)

// -- Formatting helpers -----------------------------------------------------

private val LongDateFormatter = DateTimeFormatter.ofPattern("MMMM d, yyyy", Locale.ENGLISH)

private val EuroCurrencyFormat: NumberFormat =
    NumberFormat.getCurrencyInstance(Locale.GERMANY).apply {
      currency = Currency.getInstance("EUR")
    }

private fun formatAmount(value: String): String {
  val d = value.toDoubleOrNull() ?: return value
  val formatted =
      synchronized(EuroCurrencyFormat) {
        EuroCurrencyFormat.maximumFractionDigits = 0
        EuroCurrencyFormat.format(abs(d))
      }
  val prefix = if (d < 0) "-" else "+"
  return "$prefix$formatted"
}

private fun formatLongDate(dateStr: String): String =
    try {
      LocalDate.parse(dateStr).format(LongDateFormatter)
    } catch (_: DateTimeParseException) {
      dateStr
    }

private fun formatCategoryMethod(method: CategoryMethod): String =
    when (method) {
      CategoryMethod.MANUAL -> "Manually categorized"
      CategoryMethod.RULE -> "Categorized by rule"
      CategoryMethod.LLM -> "Categorized by AI"
    }

// -- Detail field model -----------------------------------------------------

private data class DetailField(
    val label: String,
    val value: String?,
)

private const val MAX_KEY_LENGTH = 39

private fun splitRemittanceSegments(segments: List<String>): List<DetailField> {
  return segments
      .filter { it.isNotBlank() }
      .map { segment ->
        val colonIndex = segment.indexOf(": ")
        if (colonIndex in 1..MAX_KEY_LENGTH) {
          DetailField(segment.substring(0, colonIndex), segment.substring(colonIndex + 2))
        } else {
          DetailField("Remittance", segment)
        }
      }
}

private fun buildDetailFields(txn: Transaction): List<DetailField> {
  val remittanceFields = splitRemittanceSegments(txn.remittanceInformation)

  val originalAmount =
      if (txn.originalAmount != null) {
        listOfNotNull(txn.originalAmount, txn.originalCurrency).joinToString(" ")
      } else {
        null
      }

  val isoCode =
      txn.bankTransactionCodeCode?.let { code ->
        txn.bankTransactionCodeSubCode?.let { "$code-$it" } ?: code
      }

  val balanceAfter =
      txn.balanceAfterTransaction?.let { amount ->
        txn.balanceAfterTransactionCurrency?.let { "$amount $it" } ?: amount
      }

  return remittanceFields +
      listOf(
          DetailField("Counterparty", txn.counterpartyName),
          DetailField("IBAN", txn.counterpartyIban),
          DetailField("BIC", txn.counterpartyBic),
          DetailField("Original amount", originalAmount),
          DetailField("Bank code", txn.bankTransactionCode),
          DetailField("MCC", txn.merchantCategoryCode),
          DetailField("ISO 20022", isoCode),
          DetailField("Reference", txn.referenceNumber),
          DetailField("Note", txn.note),
          DetailField(
              "FX rate",
              txn.exchangeRate?.let { rate ->
                val unit = txn.exchangeRateUnitCurrency?.let { " $it" } ?: ""
                val type = txn.exchangeRateType?.let { " ($it)" } ?: ""
                "$rate$unit$type"
              },
          ),
          DetailField("FX contract", txn.exchangeRateContractId),
          DetailField("Balance after", balanceAfter),
          DetailField("Correlation", txn.correlationType),
      )
}

// -- Root composable --------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TransactionDetailScreen(
    state: TransactionsUiState,
    viewModel: TransactionsViewModel,
    onBack: () -> Unit,
) {
  val txn = state.selectedTransaction
  if (txn == null) {
    Scaffold(
        topBar = {
          TopAppBar(
              title = { Text("Transaction") },
              navigationIcon = {
                IconButton(onClick = onBack) {
                  Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
              },
              colors =
                  TopAppBarDefaults.topAppBarColors(
                      containerColor = MaterialTheme.colorScheme.surface,
                  ),
          )
        },
    ) { padding ->
      Box(
          modifier = Modifier.padding(padding).fillMaxSize(),
          contentAlignment = Alignment.Center,
      ) {
        if (state.detailLoading) {
          CircularProgressIndicator()
        } else {
          val error = state.error
          if (error != null) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
              Text(
                  text = error,
                  color = MaterialTheme.colorScheme.error,
              )
              Spacer(modifier = Modifier.height(12.dp))
              TextButton(onClick = onBack) { Text("Go back") }
            }
          }
        }
      }
    }
    return
  }

  var showPicker by remember { mutableStateOf(false) }

  val fields = buildDetailFields(txn)
  val presentFields = fields.filter { it.value != null }
  val missingFields = fields.filter { it.value == null }

  Scaffold(
      topBar = {
        TopAppBar(
            title = { Text("Transaction") },
            navigationIcon = {
              IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
              }
            },
            colors =
                TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
        )
      },
  ) { innerPadding ->
    LazyColumn(
        modifier = Modifier.padding(innerPadding).fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
      // Error banner
      val error = state.error
      if (error != null) {
        item {
          Text(
              text = error,
              color = MaterialTheme.colorScheme.error,
              style = MaterialTheme.typography.bodyMedium,
              modifier = Modifier.fillMaxWidth(),
          )
        }
      }

      // Hero section: amount + merchant + date
      item { AmountHeader(txn) }

      // Details table
      item { DetailsCard(presentFields, missingFields) }

      // AI Suggestion card
      if (txn.suggestedCategory != null) {
        item {
          val canAccept =
              state.categories.any {
                it.displayName.equals(txn.suggestedCategory, ignoreCase = true) ||
                    it.name.equals(txn.suggestedCategory, ignoreCase = true)
              }
          SuggestionCard(
              txn = txn,
              canAccept = canAccept,
              categorizing = state.categorizing,
              onAccept = viewModel::acceptSuggestion,
          )
        }
      }

      // Category action
      item {
        CategorySection(
            txn = txn,
            categories = state.categories,
            onOpenPicker = { showPicker = true },
        )
      }
    }
  }

  // Category picker bottom sheet
  if (showPicker) {
    DetailCategoryPickerSheet(
        state = state,
        filteredCategories = viewModel.filteredCategories(),
        onSearchChange = viewModel::updateCategorySearch,
        onCategorySelected = viewModel::categorize,
        onDismiss = {
          showPicker = false
          viewModel.updateCategorySearch("")
        },
    )
  }
}

// -- Hero amount header -----------------------------------------------------

@Composable
private fun AmountHeader(txn: Transaction) {
  val amount = txn.amount.toDoubleOrNull() ?: 0.0
  val amountColor = if (amount < 0) ExpenseColor else IncomeColor
  val merchant = txn.merchantName.ifEmpty { txn.remittanceInformation.firstOrNull() ?: "" }

  Column(
      modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
      horizontalAlignment = Alignment.CenterHorizontally,
  ) {
    Text(
        text = formatAmount(txn.amount),
        style = MaterialTheme.typography.headlineLarge,
        fontWeight = FontWeight.Bold,
        color = amountColor,
    )
    Spacer(modifier = Modifier.height(4.dp))
    if (merchant.isNotEmpty()) {
      Text(
          text = merchant,
          style = MaterialTheme.typography.titleMedium,
          maxLines = 2,
          overflow = TextOverflow.Ellipsis,
      )
    }
    Text(
        text = formatLongDate(txn.postedDate),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}

// -- Details card with table rows -------------------------------------------

@Composable
private fun DetailsCard(
    presentFields: List<DetailField>,
    missingFields: List<DetailField>,
) {
  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    Column(modifier = Modifier.padding(16.dp)) {
      Text(
          text = "Details",
          style = MaterialTheme.typography.titleSmall,
          fontWeight = FontWeight.Bold,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
      Spacer(modifier = Modifier.height(8.dp))

      if (presentFields.isEmpty()) {
        Text(
            text = "No additional details available",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      } else {
        presentFields.forEachIndexed { index, field ->
          DetailRow(label = field.label, value = field.value ?: "")
          if (index < presentFields.lastIndex) {
            HorizontalDivider(
                modifier = Modifier.padding(vertical = 6.dp),
                color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f),
            )
          }
        }
      }

      // Missing fields hint
      if (missingFields.isNotEmpty()) {
        Spacer(modifier = Modifier.height(12.dp))
        val missingLabels = missingFields.joinToString(", ") { it.label.lowercase() }
        Text(
            text = "No data for $missingLabels",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
        )
      }
    }
  }
}

@Composable
private fun DetailRow(label: String, value: String) {
  Row(
      modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
  ) {
    Text(
        text = label,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.width(120.dp),
    )
    Spacer(modifier = Modifier.width(12.dp))
    Text(
        text = value,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.weight(1f),
    )
  }
}

// -- AI Suggestion card -----------------------------------------------------

@Composable
private fun SuggestionCard(
    txn: Transaction,
    canAccept: Boolean,
    categorizing: Boolean,
    onAccept: () -> Unit,
) {
  val suggestion = txn.suggestedCategory ?: return

  OutlinedCard(modifier = Modifier.fillMaxWidth()) {
    Column(modifier = Modifier.padding(16.dp)) {
      Row(
          verticalAlignment = Alignment.CenterVertically,
          horizontalArrangement = Arrangement.spacedBy(8.dp),
      ) {
        Icon(
            Icons.Filled.AutoAwesome,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
        )
        Text(
            text = "AI Suggestion",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.primary,
        )
      }
      Spacer(modifier = Modifier.height(8.dp))
      Text(
          text = suggestion,
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Medium,
      )
      val justification = txn.llmJustification
      if (justification != null) {
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = justification,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
      if (canAccept) {
        Spacer(modifier = Modifier.height(12.dp))
        FilledTonalButton(
            onClick = onAccept,
            enabled = !categorizing,
            modifier = Modifier.align(Alignment.End),
        ) {
          Text("Accept suggestion")
        }
      }
    }
  }
}

// -- Category section -------------------------------------------------------

@Composable
private fun CategorySection(
    txn: Transaction,
    categories: List<DisplayCategory>,
    onOpenPicker: () -> Unit,
) {
  val categoryName = txn.categoryId?.let { id -> categories.find { it.id == id }?.displayName }

  val methodText = txn.categoryMethod?.let { formatCategoryMethod(it) }

  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    ListItem(
        headlineContent = { Text("Category") },
        supportingContent = {
          Column {
            Text(
                text = categoryName ?: "Uncategorized",
                color =
                    if (categoryName != null) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (methodText != null) {
              Text(
                  text = methodText,
                  style = MaterialTheme.typography.labelSmall,
                  color = MaterialTheme.colorScheme.onSurfaceVariant,
              )
            }
          }
        },
        trailingContent = { Icon(Icons.Default.Edit, contentDescription = "Change category") },
        modifier = Modifier.clickable(onClick = onOpenPicker),
    )
  }
}

// -- Category picker bottom sheet (picker only, no transaction details) ------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DetailCategoryPickerSheet(
    state: TransactionsUiState,
    filteredCategories: List<DisplayCategory>,
    onSearchChange: (String) -> Unit,
    onCategorySelected: (String) -> Unit,
    onDismiss: () -> Unit,
) {
  val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
  val txn = state.selectedTransaction ?: return

  ModalBottomSheet(
      onDismissRequest = onDismiss,
      sheetState = sheetState,
  ) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
    ) {
      Text(
          text = "Choose category",
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
      )

      Spacer(modifier = Modifier.height(12.dp))

      // Search field
      OutlinedTextField(
          value = state.categorySearch,
          onValueChange = onSearchChange,
          modifier = Modifier.fillMaxWidth(),
          placeholder = { Text("Search categories") },
          leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
          singleLine = true,
      )

      Spacer(modifier = Modifier.height(8.dp))

      if (state.categorizing) {
        LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        Spacer(modifier = Modifier.height(8.dp))
      }

      // Category list
      LazyColumn(
          modifier = Modifier.fillMaxWidth().weight(1f, fill = false),
      ) {
        items(filteredCategories, key = { it.id }) { category ->
          val isCurrentCategory = txn.categoryId == category.id
          ListItem(
              headlineContent = {
                Text(
                    text = category.name,
                    fontWeight = if (category.depth == 0) FontWeight.Medium else FontWeight.Normal,
                )
              },
              supportingContent = category.parentName?.let { name -> { Text(name) } },
              leadingContent =
                  if (category.depth > 0) {
                    { Spacer(modifier = Modifier.width(24.dp)) }
                  } else {
                    null
                  },
              trailingContent =
                  if (isCurrentCategory) {
                    { Icon(Icons.Default.Check, contentDescription = "Current") }
                  } else {
                    null
                  },
              modifier =
                  Modifier.clickable(enabled = !state.categorizing) {
                    onCategorySelected(category.id)
                  },
          )
        }
      }

      // Bottom padding for nav bar
      Spacer(modifier = Modifier.height(16.dp))
    }
  }
}
