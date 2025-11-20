@file:OptIn(ExperimentalStdlibApi::class)

package me.darkecho.rammingen

import android.content.Context
import android.os.Bundle
import android.text.InputType
import android.util.Log
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.edit
import androidx.preference.EditTextPreference
import androidx.preference.Preference
import androidx.preference.PreferenceDataStore
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.PreferenceManager
import com.google.crypto.tink.Aead
import com.google.crypto.tink.RegistryConfiguration
import com.google.crypto.tink.aead.AeadKeyTemplates
import com.google.crypto.tink.integration.android.AndroidKeysetManager

class SettingsActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        setContentView(R.layout.settings_activity)
        supportActionBar?.setDisplayHomeAsUpEnabled(true)
        supportFragmentManager
            .beginTransaction()
            .replace(R.id.settings, SettingsFragment())
            .commit()
    }

    override fun onSupportNavigateUp(): Boolean {
        onBackPressedDispatcher.onBackPressed()
        return true
    }

    class SettingsFragment : PreferenceFragmentCompat() {
        override fun onCreatePreferences(
            savedInstanceState: Bundle?,
            rootKey: String?,
        ) {
            val context1 = context ?: return
            preferenceManager.preferenceDataStore = EncryptedPreferenceDataStore(context1)
            setPreferencesFromResource(R.xml.preferences, rootKey)
            for (name in arrayOf("access_token", "encryption_key")) {
                val preference = findPreference<EditTextPreference>(name)
                if (preference != null) {
                    preference.summaryProvider =
                        Preference.SummaryProvider<Preference?> {
                            val value =
                                PreferenceManager
                                    .getDefaultSharedPreferences(requireContext())
                                    .getString(name, null)

                            if (value != null && value != "") {
                                "********"
                            } else {
                                "Not set"
                            }
                        }

                    preference.setOnBindEditTextListener { editText ->
                        editText.inputType =
                            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
                    }
                }
            }
        }
    }
}

class EncryptedPreferenceDataStore(
    context: Context,
) : PreferenceDataStore() {
    private val keysetHandle =
        AndroidKeysetManager
            .Builder()
            .withSharedPref(context, "default_keyset", "default_pref")
            .withKeyTemplate(AeadKeyTemplates.AES256_GCM)
            .withMasterKeyUri("android-keystore://master_key")
            .build()
            .keysetHandle
    private val aead =
        keysetHandle.getPrimitive(
            RegistryConfiguration.get(),
            Aead::class.java,
        )
//    private val Context.dataStore by preferencesDataStore("default_store")

    private val storage = PreferenceManager.getDefaultSharedPreferences(context)

    override fun putString(
        key: String,
        value: String?,
    ) {
        val encrypted =
            when (value) {
                null -> null
                "" -> ""
                else -> {
                    val valueUtf8 = value.toByteArray(Charsets.UTF_8)
                    val encrypted = aead.encrypt(valueUtf8, null)
                    encrypted.toHexString()
                }
            }
        storage.edit {
            putString(key, encrypted)
        }
    }

    override fun getString(
        key: String,
        defValue: String?,
    ): String? {
        val encryptedHex = storage.getString(key, null)
        Log.d(TAG, "storage getString: $encryptedHex")
        return when (encryptedHex) {
            null -> defValue
            "" -> ""
            else -> {
                try {
                    val encrypted = encryptedHex.hexToByteArray()
                    val valueUtf8 = aead.decrypt(encrypted, null)
                    valueUtf8.toString(Charsets.UTF_8)
                } catch (e: Exception) {
                    Log.w(TAG, "error while decrypting preferences: $e")
                    defValue
                }
            }
        }
    }

    override fun putBoolean(
        key: String,
        value: Boolean,
    ) {
        storage.edit {
            putBoolean(key, value)
        }
    }

    override fun getBoolean(
        key: String,
        defValue: Boolean,
    ): Boolean = storage.getBoolean(key, defValue)

    override fun putInt(
        key: String,
        value: Int,
    ) {
        storage.edit().apply {
            putInt(key, value)
            apply()
        }
    }

    override fun getInt(
        key: String,
        defValue: Int,
    ): Int = storage.getInt(key, defValue)

    override fun putLong(
        key: String,
        value: Long,
    ) {
        storage.edit().apply {
            putLong(key, value)
            apply()
        }
    }

    override fun getLong(
        key: String,
        defValue: Long,
    ): Long = storage.getLong(key, defValue)

    override fun putFloat(
        key: String,
        value: Float,
    ) {
        storage.edit().apply {
            putFloat(key, value)
            apply()
        }
    }

    override fun getFloat(
        key: String,
        defValue: Float,
    ): Float = storage.getFloat(key, defValue)
}
