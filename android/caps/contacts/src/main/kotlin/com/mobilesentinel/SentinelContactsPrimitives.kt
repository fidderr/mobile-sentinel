package com.mobilesentinel

import android.Manifest
import android.content.ContentResolver
import android.content.Context
import android.content.pm.PackageManager
import android.provider.ContactsContract
import android.util.Log
import androidx.core.content.ContextCompat
import org.json.JSONArray
import org.json.JSONObject

/**
 * Contact-querying primitives for the universal SDK.
 *
 * Rust calls INTO these `@JvmStatic` methods via JNI. They are thin
 * wrappers around `ContactsContract` accessed through a [ContentResolver] —
 * no orchestration, no Recipe-specific behavior. The prior raw-JNI Rust
 * implementation left `get_all` / `search` as `Unsupported` because cursor
 * iteration is awkward from JNI; this Kotlin object fills that gap while
 * keeping the same permission-check semantics.
 *
 * Hard contract (do not change signatures):
 *
 *     fun getAll(): String                 // JSON array string
 *     fun search(query: String): String    // JSON array string
 *     fun hasPermission(): Boolean
 *
 * JSON element shape per contact:
 *
 *     {
 *       "id": "...",
 *       "display_name": "...",
 *       "phone_numbers": ["...", ...],
 *       "email_addresses": ["...", ...]
 *     }
 *
 * Every method body is wrapped in try/catch and never throws. A
 * [SecurityException] is expected when `READ_CONTACTS` is not granted —
 * it is caught, logged, and a safe fallback (`"[]"` / `false`) is returned.
 */
object SentinelContactsPrimitives {
    private const val TAG = "MobileSentinel.Contacts"

    /** Cap to avoid pathological enumeration on devices with huge address books. */
    private const val MAX_CONTACTS = 500

    /**
     * Return every contact as a JSON array string. Returns `"[]"` when the
     * context is unavailable, the permission is missing, or any error occurs.
     */
    @JvmStatic
    fun getAll(): String = queryContacts(null, null)

    /**
     * Return contacts whose display name contains [query] (case-insensitive,
     * substring) as a JSON array string. Returns `"[]"` on any failure.
     */
    @JvmStatic
    fun search(query: String): String {
        val selection = "${ContactsContract.Contacts.DISPLAY_NAME} LIKE ?"
        val selectionArgs = arrayOf("%$query%")
        return queryContacts(selection, selectionArgs)
    }

    /**
     * `true` when `READ_CONTACTS` is granted. Mirrors the Rust raw-JNI
     * implementation, which used `ContextCompat.checkSelfPermission`.
     */
    @JvmStatic
    fun hasPermission(): Boolean {
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return false
            ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS) ==
                PackageManager.PERMISSION_GRANTED
        } catch (e: SecurityException) {
            Log.w(TAG, "hasPermission: SecurityException: ${e.message}")
            false
        } catch (e: Throwable) {
            Log.w(TAG, "hasPermission: ${e.message}", e)
            false
        }
    }

    /**
     * Shared enumeration path for [getAll] and [search]. Iterates
     * `ContactsContract.Contacts` and, per contact, collects phone numbers
     * and email addresses via secondary `CONTACT_ID = ?` queries.
     */
    private fun queryContacts(selection: String?, selectionArgs: Array<String>?): String {
        val result = JSONArray()
        return try {
            val context = SentinelPrimitives.getAppContext() ?: return "[]"
            val resolver = context.contentResolver

            resolver.query(
                ContactsContract.Contacts.CONTENT_URI,
                arrayOf(
                    ContactsContract.Contacts._ID,
                    ContactsContract.Contacts.DISPLAY_NAME,
                    ContactsContract.Contacts.HAS_PHONE_NUMBER,
                ),
                selection,
                selectionArgs,
                null,
            )?.use { cursor ->
                val idIndex = cursor.getColumnIndex(ContactsContract.Contacts._ID)
                val nameIndex = cursor.getColumnIndex(ContactsContract.Contacts.DISPLAY_NAME)
                val hasPhoneIndex = cursor.getColumnIndex(ContactsContract.Contacts.HAS_PHONE_NUMBER)

                while (cursor.moveToNext() && result.length() < MAX_CONTACTS) {
                    if (idIndex < 0) continue
                    val id = cursor.getString(idIndex) ?: continue
                    val displayName = if (nameIndex >= 0) cursor.getString(nameIndex) ?: "" else ""
                    val hasPhone = hasPhoneIndex >= 0 && cursor.getInt(hasPhoneIndex) > 0

                    val phoneNumbers = if (hasPhone) collectPhoneNumbers(resolver, id) else JSONArray()
                    val emailAddresses = collectEmailAddresses(resolver, id)

                    val obj = JSONObject().apply {
                        put("id", id)
                        put("display_name", displayName)
                        put("phone_numbers", phoneNumbers)
                        put("email_addresses", emailAddresses)
                    }
                    result.put(obj)
                }
            }
            result.toString()
        } catch (e: SecurityException) {
            Log.w(TAG, "queryContacts: READ_CONTACTS not granted: ${e.message}")
            "[]"
        } catch (e: Throwable) {
            Log.w(TAG, "queryContacts: ${e.message}", e)
            "[]"
        }
    }

    /** Collect all phone numbers for [contactId] from the Phone data table. */
    private fun collectPhoneNumbers(resolver: ContentResolver, contactId: String): JSONArray {
        val numbers = JSONArray()
        try {
            resolver.query(
                ContactsContract.CommonDataKinds.Phone.CONTENT_URI,
                arrayOf(ContactsContract.CommonDataKinds.Phone.NUMBER),
                "${ContactsContract.CommonDataKinds.Phone.CONTACT_ID} = ?",
                arrayOf(contactId),
                null,
            )?.use { cursor ->
                val numberIndex = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.NUMBER)
                if (numberIndex < 0) return@use
                while (cursor.moveToNext()) {
                    val number = cursor.getString(numberIndex)
                    if (!number.isNullOrBlank()) numbers.put(number)
                }
            }
        } catch (e: SecurityException) {
            Log.w(TAG, "collectPhoneNumbers($contactId): READ_CONTACTS not granted: ${e.message}")
        } catch (e: Throwable) {
            Log.w(TAG, "collectPhoneNumbers($contactId): ${e.message}", e)
        }
        return numbers
    }

    /** Collect all email addresses for [contactId] from the Email data table. */
    private fun collectEmailAddresses(resolver: ContentResolver, contactId: String): JSONArray {
        val emails = JSONArray()
        try {
            resolver.query(
                ContactsContract.CommonDataKinds.Email.CONTENT_URI,
                arrayOf(ContactsContract.CommonDataKinds.Email.ADDRESS),
                "${ContactsContract.CommonDataKinds.Email.CONTACT_ID} = ?",
                arrayOf(contactId),
                null,
            )?.use { cursor ->
                val addressIndex = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Email.ADDRESS)
                if (addressIndex < 0) return@use
                while (cursor.moveToNext()) {
                    val address = cursor.getString(addressIndex)
                    if (!address.isNullOrBlank()) emails.put(address)
                }
            }
        } catch (e: SecurityException) {
            Log.w(TAG, "collectEmailAddresses($contactId): READ_CONTACTS not granted: ${e.message}")
        } catch (e: Throwable) {
            Log.w(TAG, "collectEmailAddresses($contactId): ${e.message}", e)
        }
        return emails
    }
}
